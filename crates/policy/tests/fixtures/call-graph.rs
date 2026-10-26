//! The control#98 reproducer, with only its final stack drain extracted into `finish_graph`.
//! Kept outside the product interface and compiled by both Flux and the runtime regression tests.
use std::collections::BTreeMap;

#[derive(Debug)]
pub struct HotPath {
    pub frames: Vec<String>,
    pub sample_count: u64,
    pub share: f64,
}
#[derive(Debug)]
pub struct TopFrame {
    pub frame: String,
    pub sample_count: u64,
}
#[derive(Debug)]
pub struct Summary {
    pub total_samples: u64,
    pub waiting_samples: u64,
    pub mostly_waiting: bool,
    pub partial: bool,
    pub hot_paths: Vec<HotPath>,
    pub top_frames: Vec<TopFrame>,
}
struct Node {
    depth: usize,
    frame: String,
    samples: u64,
    children: u64,
}

/// Counts exclusive root-to-leaf paths across all threads; shares use thread samples, not time or CPU.
/// A missing graph footer marks useful output partial. No graph or inconsistent counts is an error.
pub fn summarize(text: &str, top: usize) -> Result<Summary, &'static str> {
    let (_, graph) = text
        .split_once("Call graph:\n")
        .ok_or("no sample call graph found")?;
    let mut complete = false;
    let mut stack: Vec<Node> = Vec::new();
    let mut paths = BTreeMap::new();
    for line in graph.lines() {
        if line.starts_with("Total number in stack")
            || line.starts_with("Sort by top of stack")
            || line.starts_with("Binary Images:")
        {
            complete = true;
            break;
        }
        if line.trim().is_empty() {
            continue;
        }
        add_row(line, &mut stack, &mut paths)?;
    }
    finish_graph(&mut stack, &mut paths)?;
    rank(paths, top, !complete)
}

// A separate function avoids the pinned checker's skipped-dominator-marker bug between loops.
fn finish_graph(
    stack: &mut Vec<Node>,
    paths: &mut BTreeMap<Vec<String>, u64>,
) -> Result<(), &'static str> {
    while !stack.is_empty() {
        finish_node(stack, paths)?;
    }
    Ok(())
}

fn add_row(
    line: &str,
    stack: &mut Vec<Node>,
    paths: &mut BTreeMap<Vec<String>, u64>,
) -> Result<(), &'static str> {
    let depth = line
        .find(|c: char| c.is_ascii_digit())
        .ok_or("unrecognized sample graph row")?;
    let prefix = line.get(..depth).ok_or("invalid sample indentation")?;
    if !prefix
        .chars()
        .all(|c| c.is_whitespace() || "+|!:".contains(c))
    {
        return Err("unrecognized sample graph prefix");
    }
    let row = line.get(depth..).ok_or("invalid sample graph row")?;
    let (count, frame) = row
        .split_once(char::is_whitespace)
        .ok_or("missing sample frame")?;
    let samples: u64 = count.parse().map_err(|_| "invalid sample count")?;
    let frame = frame.trim();
    let thread = frame.starts_with("Thread_");
    while stack
        .last()
        .is_some_and(|node| thread || node.depth >= depth)
    {
        finish_node(stack, paths)?;
    }
    if !thread {
        stack.push(Node {
            depth,
            frame: frame_name(frame),
            samples,
            children: 0,
        });
    }
    Ok(())
}

fn rank(
    paths: BTreeMap<Vec<String>, u64>,
    top: usize,
    partial: bool,
) -> Result<Summary, &'static str> {
    let mut total_samples = 0_u64;
    let mut waiting_samples = 0_u64;
    let mut frames = BTreeMap::new();
    let mut hot_paths = Vec::new();
    for (path, samples) in paths {
        total_samples = total_samples
            .checked_add(samples)
            .ok_or("sample total overflow")?;
        if let Some(frame) = path.last() {
            let count = frames.entry(frame.clone()).or_insert(0_u64);
            *count = count.checked_add(samples).ok_or("frame count overflow")?;
            if is_wait(frame) {
                waiting_samples = waiting_samples
                    .checked_add(samples)
                    .ok_or("waiting count overflow")?;
                continue;
            }
        }
        hot_paths.push(HotPath {
            frames: path,
            sample_count: samples,
            share: 0.0,
        });
    }
    if total_samples == 0 {
        return Err("no sampled call stacks found");
    }
    for path in &mut hot_paths {
        path.share = path.sample_count as f64 / total_samples as f64;
    }
    hot_paths.sort_by(|a, b| {
        b.sample_count
            .cmp(&a.sample_count)
            .then(a.frames.cmp(&b.frames))
    });
    hot_paths.truncate(top);
    let mut top_frames: Vec<_> = frames
        .into_iter()
        .map(|(frame, sample_count)| TopFrame {
            frame,
            sample_count,
        })
        .collect();
    top_frames.sort_by(|a, b| {
        b.sample_count
            .cmp(&a.sample_count)
            .then(a.frame.cmp(&b.frame))
    });
    top_frames.truncate(top);
    Ok(Summary {
        total_samples,
        waiting_samples,
        mostly_waiting: waiting_samples > total_samples / 2,
        partial,
        hot_paths,
        top_frames,
    })
}

/// Kernel entry alone is not a wait: reads, writes and other work are deliberately excluded.
fn is_wait(frame: &str) -> bool {
    let Some(symbol) = frame.strip_suffix(" (in libsystem_kernel.dylib)") else {
        return false;
    };
    matches!(
        symbol,
        "mach_msg_trap"
            | "mach_msg2_trap"
            | "mach_msg_overwrite_trap"
            | "semaphore_wait_trap"
            | "semaphore_timedwait_trap"
            | "semaphore_wait_signal_trap"
            | "__psynch_cvwait"
            | "__psynch_mutexwait"
            | "__psynch_rw_rdlock"
            | "__psynch_rw_wrlock"
            | "__ulock_wait"
            | "__ulock_wait2"
            | "__workq_kernreturn"
            | "kevent"
            | "kevent64"
            | "kevent_qos"
            | "__kevent64"
            | "__kevent_qos"
            | "__select"
            | "select"
            | "poll"
            | "__poll"
            | "__sigsuspend"
            | "__sigwait"
            | "__wait4"
            | "wait4"
            | "__waitid"
            | "__semwait_signal"
            | "__nanosleep"
    )
}

fn finish_node(
    stack: &mut Vec<Node>,
    paths: &mut BTreeMap<Vec<String>, u64>,
) -> Result<(), &'static str> {
    let node = stack.pop().ok_or("missing sample frame")?;
    let exclusive = node
        .samples
        .checked_sub(node.children)
        .ok_or("child sample counts exceed parent")?;
    if exclusive != 0 {
        let mut path: Vec<_> = stack.iter().map(|node| node.frame.clone()).collect();
        path.push(node.frame);
        let count = paths.entry(path).or_insert(0_u64);
        *count = count.checked_add(exclusive).ok_or("path count overflow")?;
    }
    if let Some(parent) = stack.last_mut() {
        parent.children = parent
            .children
            .checked_add(node.samples)
            .ok_or("child count overflow")?;
    }
    Ok(())
}

fn frame_name(frame: &str) -> String {
    if let Some((symbol, image)) = frame.split_once("  (in ")
        && let Some((image, _)) = image.split_once(')')
    {
        return format!("{} (in {image})", symbol.trim());
    }
    frame
        .split(" + ")
        .next()
        .unwrap_or(frame)
        .split(" [")
        .next()
        .unwrap_or(frame)
        .trim()
        .to_string()
}

# Take over whats-hot's observation, with separate disk-space ownership

hostvitals takes over [whats-hot](https://github.com/sunfmin/whats-hot)'s observation scripts and `netmon snapshot`: CPU, memory, disk activity, network, energy and looking into one Process. This decision defines the product's observation scope; the initial skeleton implements doctor and does not claim the later measurement commands already exist.

netmon's dashboard, managed downloads, speed test and `snapshot -deep` (TLS SNI through root packet capture) are not carried over. Finding what holds disk space remains diskreap's responsibility through `diskreap scan`; disk activity and disk occupancy answer different questions.

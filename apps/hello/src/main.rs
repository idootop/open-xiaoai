fn main() {
    println!("Hello from OpenWrt (ARMhf)!");
    println!("System information (via /proc/version):");
    if let Ok(version) = std::fs::read_to_string("/proc/version") {
        println!("{}", version);
    } else {
        println!("Could not read /proc/version (running in simulation?)");
    }
}

fn main() {
    std::process::exit(biomiya::driver::main_entry(
        std::env::args_os().skip(1).collect(),
    ));
}

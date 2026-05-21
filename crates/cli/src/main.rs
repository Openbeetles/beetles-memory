fn main() {
    match bm_cli::run_cli(std::env::args().skip(1)) {
        Ok(output) => println!("{output}"),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    }
}

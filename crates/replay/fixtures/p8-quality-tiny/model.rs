use std::io::{self, Read, Write};

fn main() -> io::Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let mut input = Vec::new();
    io::stdin().read_to_end(&mut input)?;

    let output = match args.as_slice() {
        [role, behavior, expected]
            if role == "reader" && behavior == "deterministic" =>
        {
            expected.as_bytes().to_vec()
        }
        [role, behavior, repeat]
            if role == "reader"
                && behavior == "seeded_repeat"
                && repeat.parse::<u32>().is_ok() =>
        {
            format!("repeat-{repeat}").into_bytes()
        }
        [role, rule, expected] if role == "judge" && rule == "exact" => {
            if input == expected.as_bytes() {
                b"correct".to_vec()
            } else {
                b"incorrect".to_vec()
            }
        }
        [role, rule, prefix] if role == "judge" && rule == "prefix" => {
            if input.starts_with(prefix.as_bytes()) {
                b"correct".to_vec()
            } else {
                b"incorrect".to_vec()
            }
        }
        [role, rule, gold]
            if role == "judge" && rule == "repeat_index" && gold == "repeat-index-bound" =>
        {
            let valid = input
                .strip_prefix(b"repeat-")
                .is_some_and(|suffix| !suffix.is_empty() && suffix.iter().all(u8::is_ascii_digit));
            if valid {
                b"correct".to_vec()
            } else {
                b"incorrect".to_vec()
            }
        }
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "fixture model command is not exact",
            ));
        }
    };

    io::stdout().lock().write_all(&output)
}

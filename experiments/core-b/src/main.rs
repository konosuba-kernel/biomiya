//! The CLI deliberately bypasses the production driver and checker.
use biomiya::{diagnostic::Diagnostic, lower, source::SourceMap, syntax, target};
use std::{env, fs, path::PathBuf, process::ExitCode};

fn execute(arguments: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let [command, input, rest @ ..] = arguments else {
        return Err("usage: core-b-prototype check FILE | emit FILE -o FILE.s".into());
    };
    let output = match (command.as_str(), rest) {
        ("check", []) => None,
        ("emit", [option, path]) if option == "-o" => Some(PathBuf::from(path)),
        _ => return Err("usage: core-b-prototype check FILE | emit FILE -o FILE.s".into()),
    };
    let mut sources = SourceMap::default();
    let file = sources.add(input, fs::read_to_string(input)?);
    let result = (|| -> Result<(), Diagnostic> {
        let ast = syntax::parse(file, &sources.get(file).text)?;
        let program = core_b_prototype::check::check(&[ast])?;
        if let Some(path) = output {
            let mut compilation = lower::lower(&program)?;
            let emission = target::rv64::emit(&mut compilation.types, &compilation.module)?;
            fs::write(&path, emission.assembly)
                .map_err(|error| Diagnostic::new("E_IO", Default::default(), error.to_string()))?;
            println!("emitted {}", path.display());
        } else {
            for function in &program.functions {
                println!(
                    "{} {} -> {}",
                    function.name,
                    function.symbol,
                    program.types.display(function.signature.result)
                );
            }
        }
        Ok(())
    })();
    result.map_err(|diagnostic| {
        let (line, column) = sources.line_column(diagnostic.span);
        format!("{input}:{line}:{column}: {diagnostic}").into()
    })
}

fn main() -> ExitCode {
    match execute(&env::args().skip(1).collect::<Vec<_>>()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

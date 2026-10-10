//! Optional CI helper: `limo-cad-help check`
use std::env;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        Some("check") => match limo_cad_help::check_corpus() {
            Ok(n) => {
                println!("limo-cad-help check OK ({n} pages)");
                ExitCode::SUCCESS
            }
            Err(errors) => {
                eprintln!("limo-cad-help check failed:");
                for e in errors {
                    eprintln!("- {e}");
                }
                ExitCode::FAILURE
            }
        },
        Some("search") => {
            let query = args.collect::<Vec<_>>().join(" ");
            if query.is_empty() {
                eprintln!("usage: limo-cad-help search <query>");
                return ExitCode::FAILURE;
            }
            let store = limo_cad_help::HelpStore::bundled();
            for hit in store.search(&query, None) {
                println!("{:.3}\t{}\t{}", hit.score, hit.id, hit.title);
            }
            ExitCode::SUCCESS
        }
        Some("get") => {
            let Some(id) = args.next() else {
                eprintln!("usage: limo-cad-help get <id>");
                return ExitCode::FAILURE;
            };
            match limo_cad_help::HelpStore::bundled().get(&id) {
                Ok(page) => {
                    println!("# {}\n\n{}", page.title, page.body);
                    if page.truncated {
                        eprintln!("(truncated)");
                    }
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("{e}");
                    ExitCode::FAILURE
                }
            }
        }
        _ => {
            eprintln!("usage: limo-cad-help <check|search|get> ...");
            ExitCode::FAILURE
        }
    }
}

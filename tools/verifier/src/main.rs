use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let path = match args.get(1) {
        Some(p) => p,
        None => {
            eprintln!("usage: verifier <bundle.json>");
            return ExitCode::from(2);
        }
    };
    let json = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("cannot read {path}: {e}");
            return ExitCode::from(2);
        }
    };
    match verifier::verify_bundle(&json) {
        Ok(v) => {
            println!(
                "OK  key_id={}  signed_by={}  evidence={}",
                v.key_id, v.signed_by, v.evidence_count
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("FAIL  {e}");
            ExitCode::FAILURE
        }
    }
}

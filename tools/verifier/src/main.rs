use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();

    match args.get(1).map(String::as_str) {
        Some("--anchor") => verify_anchor_cli(&args),
        Some("--help") | Some("-h") => {
            print_usage();
            ExitCode::SUCCESS
        }
        Some(path) if !path.starts_with("--") => verify_bundle_cli(path),
        _ => {
            print_usage();
            ExitCode::from(2)
        }
    }
}

fn print_usage() {
    eprintln!(
        "usage:\n  \
         verifier <bundle.json>                              # verify an export bundle\n  \
         verifier --anchor <anchor.json> <leaves.txt>        # verify a merkle anchor\n\n\
         <leaves.txt> is a newline-delimited list of 32-byte row_hash hex values,\n\
         in the same order the ledger used to build the tree (created_at ASC, id ASC\n\
         across chained tables)."
    );
}

fn verify_bundle_cli(path: &str) -> ExitCode {
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

fn verify_anchor_cli(args: &[String]) -> ExitCode {
    let anchor_path = match args.get(2) {
        Some(p) => p,
        None => {
            print_usage();
            return ExitCode::from(2);
        }
    };
    let leaves_path = match args.get(3) {
        Some(p) => p,
        None => {
            print_usage();
            return ExitCode::from(2);
        }
    };

    let anchor_json = match std::fs::read_to_string(anchor_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("cannot read {anchor_path}: {e}");
            return ExitCode::from(2);
        }
    };
    let leaves_txt = match std::fs::read_to_string(leaves_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("cannot read {leaves_path}: {e}");
            return ExitCode::from(2);
        }
    };

    let mut leaves: Vec<[u8; 32]> = Vec::new();
    for (i, line) in leaves_txt.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let bytes = match hex::decode(line) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("leaves:{}: invalid hex: {e}", i + 1);
                return ExitCode::from(2);
            }
        };
        let arr: [u8; 32] = match bytes.as_slice().try_into() {
            Ok(a) => a,
            Err(_) => {
                eprintln!("leaves:{}: row_hash must be 32 bytes", i + 1);
                return ExitCode::from(2);
            }
        };
        leaves.push(arr);
    }

    match verifier::verify_anchor(&anchor_json, &leaves) {
        Ok(v) => {
            println!(
                "OK  scope={}  leaf_count={}  root={}  key_id={}",
                v.scope, v.leaf_count, v.root_hex, v.key_id
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("FAIL  {e}");
            ExitCode::FAILURE
        }
    }
}

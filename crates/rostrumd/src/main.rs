//! `rostrumd`: see the library documentation and `docs/features/rostrumd.md`.

use std::process::ExitCode;

use rostrumd::app::{Action, Args, USAGE, run};

fn main() -> ExitCode {
    let args = match Args::parse(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::from(2);
        }
    };
    match args.action {
        Action::Help => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Action::Version => {
            println!("rostrumd {}", rostrumd::VERSION);
            return ExitCode::SUCCESS;
        }
        Action::Run => {}
    }

    rostrumd::logging::init();
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            tracing::error!(%error, "could not start the async runtime");
            return ExitCode::FAILURE;
        }
    };
    match runtime.block_on(run(args)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!(error = %chain(&error), "rostrumd stopped");
            ExitCode::FAILURE
        }
    }
}

/// An error and every cause beneath it, `outer: inner: innermost`.
fn chain(error: &dyn std::error::Error) -> String {
    let mut text = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        text.push_str(": ");
        text.push_str(&cause.to_string());
        source = cause.source();
    }
    text
}

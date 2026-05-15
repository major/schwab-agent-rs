//! Agent-oriented JSON CLI porcelain for the `schwab` crate.

pub mod account;
mod analyze;
mod auth;
mod cli;
mod equity;
mod error;
mod market;
mod options;
mod order;
mod output;
mod portfolio;
mod shared;
mod ta;
mod verify;

use std::io::{self, Write};

use clap::Parser;

use crate::cli::{Cli, Command, OptionCommand, TaCommand};
use crate::error::AppError;
use crate::output::{CommandOutput, Envelope, ErrorBody, Metadata};

/// Parses process arguments, runs the selected command, and writes JSON output.
pub async fn run_from_env() -> i32 {
    run(Cli::parse()).await
}

/// Runs a parsed CLI command and writes the structured JSON result.
pub async fn run(cli: Cli) -> i32 {
    let result = execute(cli).await;
    let mut stdout = io::stdout().lock();
    match result {
        Ok(output) => write_json(&mut stdout, &output).unwrap_or(1),
        Err(error) => {
            let code = error.exit_code();
            let output = Envelope::error(ErrorBody::from(&error));
            let write_code = write_json(&mut stdout, &output).unwrap_or(1);
            if write_code == 0 { code } else { write_code }
        }
    }
}

/// Executes a command and returns the JSON envelope value for tests and callers.
pub async fn execute(cli: Cli) -> Result<CommandOutput, AppError> {
    // Order and stock commands produce their own envelopes with dynamic command names.
    if let Command::Order(command) = &cli.command {
        return order::handle(&cli, command).await;
    }
    if let Command::Stock(command) = &cli.command {
        return equity::handle(&cli, command).await;
    }
    if let Command::Analyze(args) = &cli.command {
        let client = auth::provider(&cli)?.client().await?;
        let envelope = analyze::analyze(&client, args).await?;
        let data = envelope.data.map(serde_json::to_value).transpose()?;
        return Ok(CommandOutput {
            version: envelope.version,
            ok: envelope.ok,
            command: envelope.command,
            data,
            error: envelope.error,
            warnings: envelope.warnings,
            meta: envelope.meta,
        });
    }

    let command = cli.command_name();
    let data = match &cli.command {
        Command::Auth(command) => auth::handle(&cli, command).await?,
        Command::Market(command) => market::handle(&cli, command).await?,
        Command::Option(command) => {
            let client = auth::provider(&cli)?.client().await?;
            match command {
                OptionCommand::Expirations(args) => {
                    options::expirations::handle(&client, &args.symbol).await?
                }
                OptionCommand::Chain(args) => options::chain::handle(&client, args).await?,
                OptionCommand::Screen(args) => options::screen::handle(&client, args).await?,
                OptionCommand::Contract(args) => options::contract::handle(&client, args).await?,
            }
        }
        Command::Analyze(_) => unreachable!("handled above"),
        Command::Ta(ta_cmd) => {
            let client = auth::provider(&cli)?.client().await?;
            match ta_cmd {
                TaCommand::Dashboard(args) => ta::dashboard(&client, args).await?,
                TaCommand::ExpectedMove(args) => {
                    ta::expected_move::expected_move(&client, args).await?
                }
            }
        }
        Command::Order(_) => unreachable!("handled above"),
        Command::Portfolio(command) => portfolio::handle(&cli, command).await?,
        Command::Stock(_) => unreachable!("handled above"),
        Command::Account(command) => account::handle(&cli, command).await?,
    };
    Ok(Envelope::success(command, data, Metadata::now()))
}

/// Serializes `value` as JSON and writes it to `writer` followed by a newline.
///
/// Returns `Ok(0)` on success, or an `io::Error` if the write fails.
fn write_json<W, T>(writer: &mut W, value: &T) -> Result<i32, io::Error>
where
    W: Write,
    T: serde::Serialize,
{
    serde_json::to_writer(&mut *writer, value)?;
    writer.write_all(b"\n")?;
    Ok(0)
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use crate::cli::Cli;

    #[test]
    fn write_json_writes_json_followed_by_newline() {
        let mut buf: Vec<u8> = Vec::new();
        let value = serde_json::json!({"ok": true});
        let result = super::write_json(&mut buf, &value);

        assert_eq!(result.unwrap(), 0);
        let output = String::from_utf8(buf).unwrap();
        assert!(output.ends_with('\n'));
        let parsed: serde_json::Value = serde_json::from_str(output.trim()).unwrap();
        assert_eq!(parsed["ok"], true);
    }

    #[test]
    fn write_json_returns_error_on_write_failure() {
        struct FailWriter;
        impl std::io::Write for FailWriter {
            fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("write failed"))
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Err(std::io::Error::other("flush failed"))
            }
        }

        let mut writer = FailWriter;
        let value = serde_json::json!({"ok": true});
        assert!(super::write_json(&mut writer, &value).is_err());
    }

    #[tokio::test]
    async fn run_returns_nonzero_on_missing_token_file() {
        let cli = Cli::parse_from([
            "schwab-agent",
            "auth",
            "refresh",
            "--token",
            "/tmp/schwab-test-nonexistent-token-file",
        ]);
        let code = super::run(cli).await;
        assert_eq!(code, 3);
    }
}

/// Entry point. Delegates to [`schwab_agent_rs::run_from_env`] and exits with its return code.
#[tokio::main]
async fn main() {
    std::process::exit(schwab_agent_rs::run_from_env().await);
}

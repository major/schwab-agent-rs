use std::path::PathBuf;

use clap::{ArgGroup, Args, Parser, Subcommand};

/// Agent-oriented JSON CLI porcelain for Charles Schwab workflows.
#[derive(Debug, Parser)]
#[command(
    name = "schwab-agent",
    version,
    about = "Agent-oriented JSON CLI porcelain for Charles Schwab workflows",
    long_about = "All normal command output is compact JSON. Use --help on any command for examples and flags. Trading commands intentionally start with draft and validate workflows before placement.",
    arg_required_else_help = true,
    propagate_version = true,
    help_template = "{name} {version}\n{about-section}\n{usage-heading} {usage}\n\n{all-args}{tab}"
)]
pub struct Cli {
    /// Path to the OAuth token file.
    #[arg(long, global = true, env = "SCHWAB_TOKEN_PATH")]
    pub token: Option<PathBuf>,

    /// Schwab app client ID. Also read from SCHWAB_CLIENT_ID.
    #[arg(long, global = true, env = "SCHWAB_CLIENT_ID")]
    pub client_id: Option<String>,

    /// Schwab app client secret. Also read from SCHWAB_CLIENT_SECRET.
    #[arg(long, global = true, env = "SCHWAB_CLIENT_SECRET")]
    pub client_secret: Option<String>,

    /// OAuth callback URL registered with Schwab.
    #[arg(long, global = true, env = "SCHWAB_CALLBACK_URL")]
    pub callback_url: Option<String>,

    #[command(subcommand)]
    pub command: Command,
}

impl Cli {
    /// Returns the stable dotted command name used in JSON envelopes.
    #[must_use]
    pub fn command_name(&self) -> &'static str {
        match &self.command {
            Command::Analyze(_) => "analyze",
            Command::Auth(AuthCommand::Status) => "auth.status",
            Command::Auth(AuthCommand::Login(_)) => "auth.login",
            Command::Auth(AuthCommand::LoginUrl(_)) => "auth.login_url",
            Command::Auth(AuthCommand::Exchange(_)) => "auth.exchange",
            Command::Auth(AuthCommand::Refresh) => "auth.refresh",
            Command::Option(OptionCommand::Expirations(_)) => "option.expirations",
            Command::Option(OptionCommand::Chain(_)) => "option.chain",
            Command::Option(OptionCommand::Screen(_)) => "option.screen",
            Command::Option(OptionCommand::Contract(_)) => "option.contract",
            Command::Market(MarketCommand::History(_)) => "market.history",
            Command::Market(MarketCommand::Quote(_)) => "market.quote",
            Command::Order(_) => "order",
            Command::Portfolio(PortfolioCommand::Snapshot(_)) => "portfolio.snapshot",
            Command::Stock(_) => "stock",
            Command::Ta(TaCommand::Dashboard(_)) => "ta.dashboard",
            Command::Ta(TaCommand::ExpectedMove(_)) => "ta.expected-move",
            Command::Account(AccountCommand::Summary(_)) => "account.summary",
            Command::Account(AccountCommand::Resolve(_)) => "account.resolve",
        }
    }

    /// Returns the token path, falling back to an XDG-style default.
    #[must_use]
    pub fn token_path(&self) -> PathBuf {
        self.token.clone().unwrap_or_else(default_token_path)
    }
}

/// Top-level command groups.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Multi-symbol analysis combining quote and technical analysis dashboard.
    Analyze(AnalyzeArgs),
    /// Authentication commands for token setup and inspection.
    #[command(subcommand)]
    Auth(AuthCommand),
    /// Market-data workflows with compact JSON summaries.
    #[command(subcommand)]
    Market(MarketCommand),
    /// Option chain, screening, and contract lookup workflows.
    #[command(subcommand)]
    Option(OptionCommand),
    /// Option order construction, preview, and placement workflows.
    #[command(subcommand)]
    Order(crate::order::OrderCommand),
    /// Portfolio inspection workflows for account and position summaries.
    #[command(subcommand)]
    Portfolio(PortfolioCommand),
    /// Stock (equity) order construction, preview, and placement workflows.
    #[command(subcommand)]
    Stock(crate::equity::EquityCommand),
    /// Technical analysis indicator workflows.
    #[command(subcommand)]
    Ta(TaCommand),
    /// Account discovery and resolution workflows.
    #[command(subcommand)]
    Account(AccountCommand),
}

/// Technical analysis commands.
#[derive(Debug, Subcommand)]
pub enum TaCommand {
    /// Run all indicators for a symbol and return a category-grouped dashboard.
    Dashboard(DashboardArgs),
    /// Compute expected move from the option chain's ATM straddle.
    #[command(name = "expected-move")]
    ExpectedMove(ExpectedMoveArgs),
}

/// Arguments for `ta dashboard`.
#[derive(Debug, Args)]
pub struct DashboardArgs {
    /// Ticker symbol, for example AAPL.
    #[arg(required = true)]
    pub symbol: String,
    /// Candle interval.
    #[arg(long, default_value = "daily")]
    pub interval: String,
    /// Number of data points to return per indicator series.
    #[arg(long, default_value = "20")]
    pub points: usize,
}

/// Arguments for `ta expected-move`.
#[derive(Debug, Args)]
pub struct ExpectedMoveArgs {
    /// Ticker symbol, for example AAPL.
    #[arg(required = true)]
    pub symbol: String,
    /// Target days to expiration for the option chain.
    #[arg(long, default_value = "30")]
    pub dte: u32,
}

/// Arguments for the top-level `analyze` command.
#[derive(Debug, Args)]
pub struct AnalyzeArgs {
    /// One or more ticker symbols to analyze.
    #[arg(required = true)]
    pub symbols: Vec<String>,
    /// Candle interval for the dashboard.
    #[arg(long, default_value = "daily")]
    pub interval: String,
    /// Number of data points to return per indicator series.
    #[arg(long, default_value = "20")]
    pub points: usize,
}

/// Authentication commands.
#[derive(Debug, Subcommand)]
pub enum AuthCommand {
    /// Show local token state without printing secrets.
    Status,
    /// Full interactive login: open browser, listen for callback, exchange and save token.
    Login(LoginArgs),
    /// Build a browser authorization URL and open it in the default browser.
    LoginUrl(LoginUrlArgs),
    /// Exchange a pasted browser redirect URL for a saved token file.
    Exchange(AuthExchangeArgs),
    /// Force-refresh the saved token file.
    Refresh,
}

/// Arguments for `auth login`.
#[derive(Debug, Args)]
pub struct LoginArgs {
    /// Skip opening the authorization URL in the default browser.
    #[arg(long)]
    pub no_browser: bool,

    /// Seconds to wait for the callback before timing out.
    #[arg(long, default_value = "300")]
    pub timeout: u64,
}

/// Arguments for `auth login-url`.
#[derive(Debug, Args)]
pub struct LoginUrlArgs {
    /// Skip opening the authorization URL in the default browser.
    #[arg(long)]
    pub no_browser: bool,
}

/// Arguments for `auth exchange`.
#[derive(Debug, Args)]
pub struct AuthExchangeArgs {
    /// CSRF state returned by `auth login-url`.
    #[arg(long)]
    pub state: String,

    /// Full redirect URL copied from the browser address bar.
    #[arg(long)]
    pub redirect_url: String,
}

/// Market-data commands.
#[derive(Debug, Subcommand)]
pub enum MarketCommand {
    /// Get price history candles for a symbol.
    History(HistoryArgs),
    /// Get compact quote summaries for one or more symbols.
    Quote(QuoteArgs),
}

/// Option-chain, screening, and contract lookup commands.
#[derive(Debug, Subcommand)]
pub enum OptionCommand {
    /// Get expiration dates for an option symbol.
    Expirations(OptionExpirationsArgs),
    /// Get an option chain for a symbol.
    Chain(ChainArgs),
    /// Screen option chains with liquidity and pricing filters.
    Screen(ScreenArgs),
    /// Look up a single option contract.
    Contract(ContractArgs),
}

/// Arguments for `option expirations`.
#[derive(Debug, Args)]
pub struct OptionExpirationsArgs {
    /// Underlying symbol, for example AAPL.
    #[arg(required = true)]
    pub symbol: String,
}

/// Arguments for `option chain`.
#[derive(Debug, Args)]
pub struct ChainArgs {
    /// Underlying symbol, for example AAPL.
    #[arg(required = true)]
    pub symbol: String,

    /// Contract type filter, call, put, or all.
    #[arg(long = "type")]
    pub contract_type: Option<String>,

    /// Nearest expiration by days to expiration.
    #[arg(long, conflicts_with = "expiration")]
    pub dte: Option<i32>,

    /// Exact expiration date in YYYY-MM-DD format.
    #[arg(long, conflicts_with = "dte")]
    pub expiration: Option<String>,

    /// Minimum delta filter.
    #[arg(long)]
    pub delta_min: Option<f64>,

    /// Maximum delta filter.
    #[arg(long)]
    pub delta_max: Option<f64>,

    /// Comma-separated field list.
    #[arg(long)]
    pub fields: Option<String>,

    /// Number of strikes around at-the-money to include.
    #[arg(long)]
    pub strike_count: Option<u32>,

    /// Exact strike price.
    #[arg(long, conflicts_with_all = ["strike_min", "strike_max", "strike_count"])]
    pub strike: Option<f64>,

    /// Minimum strike price.
    #[arg(long)]
    pub strike_min: Option<f64>,

    /// Maximum strike price.
    #[arg(long)]
    pub strike_max: Option<f64>,

    /// Schwab strike range filter.
    #[arg(long)]
    pub strike_range: Option<String>,
}

/// Arguments for `option screen`.
#[derive(Debug, Args)]
pub struct ScreenArgs {
    /// Underlying symbol, for example AAPL.
    #[arg(required = true)]
    pub symbol: String,

    /// Contract type filter, call, put, or all.
    #[arg(long = "type")]
    pub contract_type: Option<String>,

    /// Minimum days to expiration.
    #[arg(long = "dte-min")]
    pub dte_min: Option<i32>,

    /// Maximum days to expiration.
    #[arg(long = "dte-max")]
    pub dte_max: Option<i32>,

    /// Exact expiration date in YYYY-MM-DD format.
    #[arg(long)]
    pub expiration: Option<String>,

    /// Minimum delta filter.
    #[arg(long)]
    pub delta_min: Option<f64>,

    /// Maximum delta filter.
    #[arg(long)]
    pub delta_max: Option<f64>,

    /// Comma-separated field list.
    #[arg(long)]
    pub fields: Option<String>,

    /// Number of strikes around at-the-money to include.
    #[arg(long)]
    pub strike_count: Option<u32>,

    /// Exact strike price.
    #[arg(long, conflicts_with_all = ["strike_min", "strike_max", "strike_count"])]
    pub strike: Option<f64>,

    /// Minimum strike price.
    #[arg(long)]
    pub strike_min: Option<f64>,

    /// Maximum strike price.
    #[arg(long)]
    pub strike_max: Option<f64>,

    /// Schwab strike range filter.
    #[arg(long)]
    pub strike_range: Option<String>,

    /// Minimum bid price.
    #[arg(long = "min-bid")]
    pub min_bid: Option<f64>,

    /// Maximum ask price.
    #[arg(long = "max-ask")]
    pub max_ask: Option<f64>,

    /// Minimum volume.
    #[arg(long = "min-volume")]
    pub min_volume: Option<u64>,

    /// Minimum open interest.
    #[arg(long = "min-oi")]
    pub min_oi: Option<u64>,

    /// Maximum spread percent.
    #[arg(long = "max-spread-pct")]
    pub max_spread_pct: Option<f64>,

    /// Minimum premium.
    #[arg(long = "min-premium")]
    pub min_premium: Option<f64>,

    /// Maximum premium.
    #[arg(long = "max-premium")]
    pub max_premium: Option<f64>,

    /// Sort field.
    #[arg(long)]
    pub sort: Option<String>,

    /// Maximum number of results.
    #[arg(long)]
    pub limit: Option<usize>,
}

/// Arguments for `option contract`.
#[derive(Debug, Args)]
#[command(group(
    ArgGroup::new("contract-side")
        .required(true)
        .args(["call", "put"])
))]
pub struct ContractArgs {
    /// Underlying symbol, for example AAPL.
    #[arg(required = true)]
    pub symbol: String,

    /// Exact expiration date in YYYY-MM-DD format.
    #[arg(long)]
    pub expiration: String,

    /// Exact strike price.
    #[arg(long)]
    pub strike: f64,

    /// Select a call contract.
    #[arg(long, conflicts_with = "put")]
    pub call: bool,

    /// Select a put contract.
    #[arg(long, conflicts_with = "call")]
    pub put: bool,
}

/// Arguments for `market history`.
#[derive(Debug, Args)]
pub struct HistoryArgs {
    /// Ticker symbol, for example AAPL.
    #[arg(required = true)]
    pub symbol: String,

    /// Period type (day, month, year, ytd).
    #[arg(long)]
    pub period_type: Option<String>,

    /// Number of periods to return.
    #[arg(long)]
    pub period: Option<i64>,

    /// Frequency type (minute, daily, weekly, monthly).
    #[arg(long)]
    pub frequency_type: Option<String>,

    /// Frequency value (e.g. 1, 5, 15).
    #[arg(long)]
    pub frequency: Option<i64>,

    /// Start date in milliseconds since epoch.
    #[arg(long)]
    pub from: Option<i64>,

    /// End date in milliseconds since epoch.
    #[arg(long)]
    pub to: Option<i64>,

    /// Include extended-hours data.
    #[arg(long)]
    pub extended_hours: bool,
}

/// Arguments for `market quote`.
#[derive(Debug, Args)]
pub struct QuoteArgs {
    /// Symbols to quote, for example AAPL MSFT $SPX.
    #[arg(required = true)]
    pub symbols: Vec<String>,

    /// Comma-separated output fields. Defaults to req,sym,bid,ask,last,mark,chg,pct,vol,err.
    #[arg(long, conflicts_with = "all_fields")]
    pub fields: Option<String>,

    /// Return the full detailed quote object instead of compact rows.
    #[arg(long, conflicts_with = "fields")]
    pub all_fields: bool,

    /// Schwab quote field groups to request from the API, for example quote,reference.
    #[arg(long)]
    pub api_fields: Option<String>,
}

/// Portfolio commands.
#[derive(Debug, Subcommand)]
pub enum PortfolioCommand {
    /// Get compact account and position summaries for portfolio triage.
    Snapshot(PortfolioSnapshotArgs),
}

/// Arguments for `portfolio snapshot`.
#[derive(Debug, Args)]
pub struct PortfolioSnapshotArgs {
    /// Include individual positions in each account summary.
    #[arg(long)]
    pub positions: bool,
}

/// Account commands.
#[derive(Debug, Subcommand)]
pub enum AccountCommand {
    /// Get compact account summaries with balance and optional position data.
    Summary(AccountSummaryArgs),
    /// Resolve an account hash or nickname to its canonical account hash.
    Resolve(AccountResolveArgs),
}

/// Arguments for `account summary`.
#[derive(Debug, Args)]
pub struct AccountSummaryArgs {
    /// Include individual positions in each account summary.
    #[arg(long)]
    pub positions: bool,
}

/// Arguments for `account resolve`.
#[derive(Debug, Args)]
pub struct AccountResolveArgs {
    /// Account hash or nickname to resolve.
    pub selector: String,
}

fn default_token_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("schwab-agent-rs")
        .join("token.json")
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use clap::{CommandFactory, Parser};

    use super::{AccountCommand, Cli, Command, MarketCommand, TaCommand, default_token_path};

    #[test]
    fn command_tree_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn command_name_auth_status() {
        let cli = Cli::parse_from(["schwab-agent", "auth", "status"]);
        assert_eq!(cli.command_name(), "auth.status");
    }

    #[test]
    fn command_name_analyze() {
        let cli = Cli::parse_from(["schwab-agent", "analyze", "AAPL"]);
        assert_eq!(cli.command_name(), "analyze");
    }

    #[test]
    fn command_name_auth_login_url() {
        let cli = Cli::parse_from(["schwab-agent", "auth", "login-url"]);
        assert_eq!(cli.command_name(), "auth.login_url");
    }

    #[test]
    fn command_name_auth_exchange() {
        let cli = Cli::parse_from([
            "schwab-agent",
            "auth",
            "exchange",
            "--state",
            "abc",
            "--redirect-url",
            "https://example.com",
        ]);
        assert_eq!(cli.command_name(), "auth.exchange");
    }

    #[test]
    fn command_name_auth_refresh() {
        let cli = Cli::parse_from(["schwab-agent", "auth", "refresh"]);
        assert_eq!(cli.command_name(), "auth.refresh");
    }

    #[test]
    fn command_name_market_history() {
        let cli = Cli::parse_from(["schwab-agent", "market", "history", "AAPL"]);
        assert_eq!(cli.command_name(), "market.history");
    }

    #[test]
    fn command_name_market_history_with_all_flags() {
        let cli = Cli::parse_from([
            "schwab-agent",
            "market",
            "history",
            "AAPL",
            "--period-type",
            "month",
            "--period",
            "3",
            "--frequency-type",
            "daily",
            "--frequency",
            "1",
            "--from",
            "1735689600000",
            "--to",
            "1743379200000",
            "--extended-hours",
        ]);
        assert_eq!(cli.command_name(), "market.history");
    }

    #[test]
    fn command_name_market_quote() {
        let cli = Cli::parse_from(["schwab-agent", "market", "quote", "AAPL"]);
        assert_eq!(cli.command_name(), "market.quote");
    }

    #[test]
    fn market_quote_fields_parse_output_and_api_fields() {
        let cli = Cli::parse_from([
            "schwab-agent",
            "market",
            "quote",
            "AAPL",
            "--fields",
            "sym,last",
            "--api-fields",
            "quote,reference",
        ]);

        let Command::Market(MarketCommand::Quote(args)) = cli.command else {
            panic!("expected market quote command");
        };
        assert_eq!(args.fields.as_deref(), Some("sym,last"));
        assert_eq!(args.api_fields.as_deref(), Some("quote,reference"));
        assert!(!args.all_fields);
    }

    #[test]
    fn market_quote_all_fields_parses() {
        let cli = Cli::parse_from(["schwab-agent", "market", "quote", "AAPL", "--all-fields"]);

        let Command::Market(MarketCommand::Quote(args)) = cli.command else {
            panic!("expected market quote command");
        };
        assert!(args.all_fields);
        assert!(args.fields.is_none());
    }

    #[test]
    fn command_name_option_expirations() {
        let cli = Cli::parse_from(["schwab-agent", "option", "expirations", "AAPL"]);
        assert_eq!(cli.command_name(), "option.expirations");
    }

    #[test]
    fn command_name_option_chain() {
        let cli = Cli::parse_from(["schwab-agent", "option", "chain", "AAPL"]);
        assert_eq!(cli.command_name(), "option.chain");
    }

    #[test]
    fn command_name_option_screen() {
        let cli = Cli::parse_from(["schwab-agent", "option", "screen", "AAPL"]);
        assert_eq!(cli.command_name(), "option.screen");
    }

    #[test]
    fn command_name_option_contract() {
        let cli = Cli::parse_from([
            "schwab-agent",
            "option",
            "contract",
            "AAPL",
            "--expiration",
            "2026-01-17",
            "--strike",
            "200",
            "--call",
        ]);
        assert_eq!(cli.command_name(), "option.contract");
    }

    #[test]
    fn command_name_portfolio_snapshot() {
        let cli = Cli::parse_from(["schwab-agent", "portfolio", "snapshot"]);
        assert_eq!(cli.command_name(), "portfolio.snapshot");
    }

    #[test]
    fn command_name_account_summary() {
        let cli = Cli::parse_from(["schwab-agent", "account", "summary"]);
        assert_eq!(cli.command_name(), "account.summary");
    }

    #[test]
    fn command_name_account_summary_with_positions() {
        let cli = Cli::parse_from(["schwab-agent", "account", "summary", "--positions"]);
        assert_eq!(cli.command_name(), "account.summary");
    }

    #[test]
    fn command_name_account_resolve() {
        let cli = Cli::parse_from(["schwab-agent", "account", "resolve", "Trading"]);
        assert_eq!(cli.command_name(), "account.resolve");
    }

    #[test]
    fn command_name_ta_dashboard() {
        let cli = Cli::parse_from(["schwab-agent", "ta", "dashboard", "AAPL"]);
        assert_eq!(cli.command_name(), "ta.dashboard");
    }

    #[test]
    fn command_name_ta_expected_move() {
        let cli = Cli::parse_from(["schwab-agent", "ta", "expected-move", "AAPL"]);
        assert_eq!(cli.command_name(), "ta.expected-move");
    }

    #[test]
    fn parse_account_summary_no_flags() {
        let cli = Cli::parse_from(["schwab-agent", "account", "summary"]);

        let Command::Account(AccountCommand::Summary(args)) = cli.command else {
            panic!("expected account summary command");
        };
        assert!(!args.positions);
    }

    #[test]
    fn parse_account_summary_positions() {
        let cli = Cli::parse_from(["schwab-agent", "account", "summary", "--positions"]);

        let Command::Account(AccountCommand::Summary(args)) = cli.command else {
            panic!("expected account summary command");
        };
        assert!(args.positions);
    }

    #[test]
    fn parse_account_resolve_selector() {
        let cli = Cli::parse_from(["schwab-agent", "account", "resolve", "Trading"]);

        let Command::Account(AccountCommand::Resolve(args)) = cli.command else {
            panic!("expected account resolve command");
        };
        assert_eq!(args.selector, "Trading");
    }

    #[test]
    fn parse_account_resolve_requires_selector() {
        let result = Cli::try_parse_from(["schwab-agent", "account", "resolve"]);
        assert!(result.is_err());
    }

    #[test]
    fn parse_ta_dashboard_defaults() {
        let cli = Cli::parse_from(["schwab-agent", "ta", "dashboard", "AAPL"]);

        let Command::Ta(TaCommand::Dashboard(args)) = cli.command else {
            panic!("expected ta dashboard command");
        };
        assert_eq!(args.symbol, "AAPL");
        assert_eq!(args.interval, "daily");
        assert_eq!(args.points, 20);
    }

    #[test]
    fn parse_ta_dashboard_custom_interval_and_points() {
        let cli = Cli::parse_from([
            "schwab-agent",
            "ta",
            "dashboard",
            "AAPL",
            "--interval",
            "weekly",
            "--points",
            "10",
        ]);

        let Command::Ta(TaCommand::Dashboard(args)) = cli.command else {
            panic!("expected ta dashboard command");
        };
        assert_eq!(args.symbol, "AAPL");
        assert_eq!(args.interval, "weekly");
        assert_eq!(args.points, 10);
    }

    #[test]
    fn parse_ta_expected_move_defaults() {
        let cli = Cli::parse_from(["schwab-agent", "ta", "expected-move", "AAPL"]);

        let Command::Ta(TaCommand::ExpectedMove(args)) = cli.command else {
            panic!("expected ta expected-move command");
        };
        assert_eq!(args.symbol, "AAPL");
        assert_eq!(args.dte, 30);
    }

    #[test]
    fn parse_ta_expected_move_custom_dte() {
        let cli = Cli::parse_from(["schwab-agent", "ta", "expected-move", "AAPL", "--dte", "45"]);

        let Command::Ta(TaCommand::ExpectedMove(args)) = cli.command else {
            panic!("expected ta expected-move command");
        };
        assert_eq!(args.symbol, "AAPL");
        assert_eq!(args.dte, 45);
    }

    #[test]
    fn parse_analyze_multiple_symbols() {
        let cli = Cli::parse_from(["schwab-agent", "analyze", "AAPL", "MSFT"]);

        let Command::Analyze(args) = cli.command else {
            panic!("expected analyze command");
        };
        assert_eq!(args.symbols, ["AAPL", "MSFT"]);
        assert_eq!(args.interval, "daily");
        assert_eq!(args.points, 20);
    }

    #[test]
    fn parse_analyze_custom_interval_and_points() {
        let cli = Cli::parse_from([
            "schwab-agent",
            "analyze",
            "AAPL",
            "--interval",
            "daily",
            "--points",
            "5",
        ]);

        let Command::Analyze(args) = cli.command else {
            panic!("expected analyze command");
        };
        assert_eq!(args.symbols, ["AAPL"]);
        assert_eq!(args.interval, "daily");
        assert_eq!(args.points, 5);
    }

    #[test]
    fn token_path_uses_explicit_flag() {
        let cli = Cli::parse_from([
            "schwab-agent",
            "--token",
            "/custom/path/token.json",
            "auth",
            "status",
        ]);
        assert_eq!(cli.token_path(), PathBuf::from("/custom/path/token.json"));
    }

    #[test]
    fn default_token_path_ends_with_expected_suffix() {
        let path = default_token_path();
        assert!(path.ends_with("schwab-agent-rs/token.json"));
    }
}

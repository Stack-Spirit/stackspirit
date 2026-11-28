//! Built-in CLI & MCP presets. Pure data: no secret, token or account value.

use crate::tools::model::{CliSpec, McpSpec, McpTransport, ToolCategory, ToolEntry};

fn cli(binary: &str, install: &str, auth: &str) -> Option<CliSpec> {
    Some(CliSpec {
        binary: binary.to_owned(),
        install_cmd: install.to_owned(),
        auth_cmd: auth.to_owned(),
    })
}

fn http(url: &str) -> Option<McpSpec> {
    Some(McpSpec {
        transport: McpTransport::Http,
        command: String::new(),
        args: Vec::new(),
        url: url.to_owned(),
    })
}

fn stdio(command: &str, args: &[&str]) -> Option<McpSpec> {
    Some(McpSpec {
        transport: McpTransport::Stdio,
        command: command.to_owned(),
        args: args.iter().map(|a| (*a).to_owned()).collect(),
        url: String::new(),
    })
}

fn entry(
    id: &str,
    name: &str,
    category: ToolCategory,
    description: &str,
    cli: Option<CliSpec>,
    mcp: Option<McpSpec>,
) -> ToolEntry {
    ToolEntry {
        id: id.to_owned(),
        name: name.to_owned(),
        category,
        description: description.to_owned(),
        cli,
        mcp,
        builtin: true,
    }
}

/// The built-in presets (19 entries). "+ Add custom CLI" and "+ Add custom MCP" are
/// form actions, not entries; custom entries are saved through
/// [`crate::tools::ToolRegistry::save_custom`].
#[must_use]
pub fn builtin_tools() -> Vec<ToolEntry> {
    use ToolCategory::{BrowserDocs, CodeIssues, CommunicationCalendar, Databases, DeployCloud};
    vec![
        entry(
            "github",
            "GitHub",
            CodeIssues,
            "Repositories, pull requests and issues.",
            cli("gh", "brew install gh", "gh auth login"),
            http("https://api.githubcopilot.com/mcp/"),
        ),
        entry(
            "vercel",
            "Vercel",
            DeployCloud,
            "Deploy and inspect Vercel projects.",
            cli("vercel", "npm install -g vercel", "vercel login"),
            http("https://mcp.vercel.com"),
        ),
        entry(
            "cloudflare",
            "Cloudflare",
            DeployCloud,
            "Workers, Pages, DNS and R2 through wrangler.",
            cli("wrangler", "npm install -g wrangler", "wrangler login"),
            http("https://mcp.cloudflare.com/mcp"),
        ),
        entry(
            "stripe",
            "Stripe",
            DeployCloud,
            "Payments, customers and webhooks.",
            cli(
                "stripe",
                "brew install stripe/stripe-cli/stripe",
                "stripe login",
            ),
            http("https://mcp.stripe.com"),
        ),
        entry(
            "supabase",
            "Supabase",
            Databases,
            "Postgres, auth and storage projects.",
            cli(
                "supabase",
                "brew install supabase/tap/supabase",
                "supabase login",
            ),
            http("https://mcp.supabase.com/mcp"),
        ),
        entry(
            "linear",
            "Linear",
            CodeIssues,
            "Issues, projects and cycles.",
            None,
            http("https://mcp.linear.app/mcp"),
        ),
        entry(
            "slack",
            "Slack",
            CommunicationCalendar,
            "Read and post in Slack channels.",
            None,
            http("https://mcp.slack.com/mcp"),
        ),
        entry(
            "gmail",
            "Gmail",
            CommunicationCalendar,
            "Search and draft email.",
            None,
            http("https://gmailmcp.googleapis.com/mcp/v1"),
        ),
        entry(
            "google-calendar",
            "Google Calendar",
            CommunicationCalendar,
            "Read and create calendar events.",
            None,
            http("https://calendarmcp.googleapis.com/mcp/v1"),
        ),
        entry(
            "notion",
            "Notion",
            BrowserDocs,
            "Search and edit Notion pages.",
            None,
            http("https://mcp.notion.com/mcp"),
        ),
        entry(
            "sentry",
            "Sentry",
            CodeIssues,
            "Errors, releases and performance traces.",
            None,
            http("https://mcp.sentry.dev/mcp"),
        ),
        entry(
            "postgres",
            "Postgres",
            Databases,
            "Query a Postgres database. The connection string stays in the secret vault.",
            None,
            stdio("npx", &["-y", "@modelcontextprotocol/server-postgres"]),
        ),
        entry(
            "playwright",
            "Playwright",
            BrowserDocs,
            "Drive a real browser for testing and scraping.",
            None,
            stdio("npx", &["-y", "@playwright/mcp@latest"]),
        ),
        entry(
            "context7",
            "Context7",
            BrowserDocs,
            "Up-to-date library documentation.",
            None,
            stdio("npx", &["-y", "@upstash/context7-mcp"]),
        ),
        entry(
            "docker",
            "Docker",
            DeployCloud,
            "Build and run containers.",
            cli("docker", "brew install --cask docker", "docker login"),
            None,
        ),
        entry(
            "aws",
            "AWS",
            DeployCloud,
            "Amazon Web Services through the aws CLI.",
            cli("aws", "brew install awscli", "aws sso login"),
            None,
        ),
        entry(
            "gcloud",
            "Google Cloud",
            DeployCloud,
            "Google Cloud through the gcloud CLI.",
            cli(
                "gcloud",
                "brew install --cask google-cloud-sdk",
                "gcloud auth login",
            ),
            None,
        ),
        entry(
            "flyio",
            "Fly.io",
            DeployCloud,
            "Deploy apps with flyctl.",
            cli("flyctl", "brew install flyctl", "flyctl auth login"),
            None,
        ),
        entry(
            "hetzner",
            "Hetzner",
            DeployCloud,
            "Hetzner Cloud servers through hcloud.",
            cli("hcloud", "brew install hcloud", "hcloud context create"),
            None,
        ),
    ]
}

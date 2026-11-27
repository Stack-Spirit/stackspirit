//! The read-only fleet dashboard tool and its MCP Apps status resource.
//!
//! Both surfaces render the same zero-secret data from the live snapshot and capacity
//! report. [`render_markdown`] returns a compact Markdown document with ASCII gauges for a
//! model or an orchestrator; [`render_html`] returns a self-contained, zero-CDN Meadow
//! widget for an MCP Apps-capable client. Every dynamic string is escaped before it is
//! written, and no token, lease or secret value is ever read or rendered.

use serde_json::{Value, json};
use vowp_protocol::{
    BrakeCode, CapacityReport, CapacityTarget, FleetStatus, HostSample, ProjectState, Snapshot, TargetMode,
};

/// URI of the MCP Apps fleet status resource.
pub(crate) const RESOURCE_URI: &str = "ui://stackspirit/fleet-status";
/// MIME type that marks the resource as a self-contained MCP App.
pub(crate) const RESOURCE_MIME: &str = "text/html;profile=mcp-app";

/// Returns the MCP `resources/list` result.
pub(crate) fn resources_list() -> Value {
    json!({
        "resources": [{
            "uri": RESOURCE_URI,
            "name": "StackSpirit fleet status",
            "title": "StackSpirit fleet status",
            "description": "Live VPS state, worker slots, active jobs, capacity bars and open inbox as a self-contained Meadow HTML widget. Zero-secret and read-only.",
            "mimeType": RESOURCE_MIME,
        }]
    })
}

/// Returns the MCP `resources/read` result for one rendered widget.
pub(crate) fn resource_contents(html: &str) -> Value {
    json!({
        "contents": [{
            "uri": RESOURCE_URI,
            "mimeType": RESOURCE_MIME,
            "text": html,
        }]
    })
}

/// Values shared by the Markdown and HTML renderers.
struct Facts<'a> {
    status: &'a FleetStatus,
    host: Option<&'a HostSample>,
    busy: u32,
    target: u32,
    waiting: u32,
    active: u32,
    ready: u32,
    blocked: u32,
    inbox: u32,
}

impl<'a> Facts<'a> {
    fn new(status: &'a FleetStatus, capacity: Option<&'a CapacityReport>) -> Self {
        let snapshot = &status.snapshot;
        let active = u32::try_from(
            snapshot
                .projects
                .iter()
                .map(|project| project.running.len())
                .sum::<usize>(),
        )
        .unwrap_or(u32::MAX);
        let ready = snapshot.projects.iter().fold(0_u32, |total, project| {
            total.saturating_add(project.jobs_by_state.get("ready").copied().unwrap_or(0))
        });
        let blocked = u32::try_from(
            snapshot
                .projects
                .iter()
                .map(|project| project.blocked.len())
                .sum::<usize>(),
        )
        .unwrap_or(u32::MAX);
        let inbox = u32::try_from(snapshot.inbox.len()).unwrap_or(u32::MAX);
        let waiting = u32::try_from(capacity.map_or(0, |report| report.waiting.len())).unwrap_or(u32::MAX);
        let host = snapshot
            .capacity
            .measured
            .as_ref()
            .or_else(|| capacity.and_then(|report| report.host.as_ref()));
        Self {
            status,
            host,
            busy: active,
            target: snapshot.capacity.target.target_workers,
            waiting,
            active,
            ready,
            blocked,
            inbox,
        }
    }

    fn snapshot(&self) -> &Snapshot {
        &self.status.snapshot
    }

    fn target(&self) -> &CapacityTarget {
        &self.status.snapshot.capacity.target
    }

    fn target_mode(&self) -> &'static str {
        target_mode_name(self.target().mode)
    }

    fn uptime_secs(&self) -> Option<u64> {
        let supervisor = &self.snapshot().supervisor;
        supervisor
            .started_at
            .map(|started| self.snapshot().generated_at.since(started).as_secs())
    }

    fn last_tick_age_secs(&self) -> Option<u64> {
        let snapshot = self.snapshot();
        snapshot
            .supervisor
            .last_tick
            .map(|last| snapshot.generated_at.since(last).as_secs())
    }

    fn supervisor_summary(&self) -> String {
        let supervisor = &self.snapshot().supervisor;
        if !supervisor.running {
            return if supervisor.paused {
                "paused".to_owned()
            } else {
                "not running".to_owned()
            };
        }
        let mut parts = vec!["running".to_owned()];
        if let Some(pid) = supervisor.pid {
            parts.push(format!("pid {pid}"));
        }
        if let Some(uptime) = self.uptime_secs() {
            parts.push(format!("up {}", format_duration(uptime)));
        }
        parts.push(format!("tick {}s", supervisor.tick_secs));
        if let Some(age) = self.last_tick_age_secs() {
            parts.push(format!("last tick {} ago", format_duration(age)));
        }
        if supervisor.fail_streak > 0 {
            parts.push(format!("fail streak {}", supervisor.fail_streak));
        }
        if !supervisor.version.is_empty() {
            parts.push(format!("v{}", code_text(&supervisor.version)));
        }
        parts.join(" · ")
    }

    fn worker_percent(&self) -> u8 {
        percent(u64::from(self.busy), u64::from(self.target))
    }

    fn workers_summary(&self) -> String {
        let target = self.target();
        let mut parts = vec![format!("{}/{} busy", self.busy, self.target)];
        let mut mode = format!("{} target", self.target_mode());
        if let Some(advised) = target.advised_workers {
            mode.push_str(&format!(" (advised {advised}"));
            if target.is_override {
                mode.push_str(", override");
            }
            mode.push(')');
        } else if target.is_override {
            mode.push_str(" (override)");
        }
        parts.push(mode);
        if self.waiting > 0 {
            parts.push(format!("{} waiting", self.waiting));
        }
        parts.join(" · ")
    }

    fn cpu_percent(&self) -> u8 {
        let Some(host) = self.host else {
            return 0;
        };
        let cores = host
            .cpu_quota_cores
            .filter(|quota| *quota > 0.0)
            .unwrap_or(f64::from(host.cpu_count.max(1)));
        let used = (host.load1 / cores * 100.0).clamp(0.0, 100.0);
        u8::try_from(used.round() as u32).unwrap_or(100)
    }

    fn cpu_note(&self) -> String {
        let Some(host) = self.host else {
            return "no host sample yet".to_owned();
        };
        format!(
            "{}% · load {:.2} / {:.2} / {:.2} · {} cores",
            self.cpu_percent(),
            host.load1,
            host.load5,
            host.load15,
            host.cpu_count
        )
    }

    fn ram_percent(&self) -> u8 {
        let Some(host) = self.host else {
            return 0;
        };
        percent(
            host.mem_total_bytes.saturating_sub(host.mem_available_bytes),
            host.mem_total_bytes,
        )
    }

    fn ram_note(&self) -> String {
        let Some(host) = self.host else {
            return "no host sample yet".to_owned();
        };
        format!(
            "{}% used · {} free of {}",
            self.ram_percent(),
            crate::cmd::capacity::format_bytes(host.mem_available_bytes),
            crate::cmd::capacity::format_bytes(host.mem_total_bytes)
        )
    }

    fn disk_percent(&self) -> u8 {
        let Some(host) = self.host else {
            return 0;
        };
        let reserve = self.snapshot().capacity.min_disk_bytes;
        if reserve == 0 {
            return 100;
        }
        // The snapshot carries no total disk size, so the gauge is the headroom over the
        // reserve floor: a full bar means at least the reserve is still free.
        percent(host.disk_free_home_bytes.min(reserve), reserve)
    }

    fn disk_note(&self) -> String {
        let Some(host) = self.host else {
            return "no host sample yet".to_owned();
        };
        format!(
            "{} free · {} reserve",
            crate::cmd::capacity::format_bytes(host.disk_free_home_bytes),
            crate::cmd::capacity::format_bytes(self.snapshot().capacity.min_disk_bytes)
        )
    }
}

/// Renders the Markdown + ASCII dashboard from the live snapshot and capacity report.
pub(crate) fn render_markdown(status: &FleetStatus, capacity: Option<&CapacityReport>) -> String {
    let facts = Facts::new(status, capacity);
    let snapshot = facts.snapshot();
    let mut out = String::new();
    out.push_str(&format!(
        "# StackSpirit fleet — {}\n\n",
        md_text(snapshot.home.as_str())
    ));
    let supervisor_state = if snapshot.supervisor.running {
        "supervisor running"
    } else if snapshot.supervisor.paused {
        "supervisor paused"
    } else {
        "supervisor stopped"
    };
    out.push_str(&format!(
        "{supervisor_state} · source {} · fetched {} · {}\n\n",
        md_text(&status.source),
        status.fetched_at.as_rfc3339(),
        if status.stale { "**stale**" } else { "fresh" },
    ));
    out.push_str("```text\n");
    out.push_str(&format!("supervisor  {}\n", facts.supervisor_summary()));
    out.push_str(&format!(
        "workers     {}  {}\n",
        gauge(facts.worker_percent()),
        facts.workers_summary()
    ));
    out.push_str(&format!(
        "cpu         {}  {}\n",
        gauge(facts.host.map_or(0, |_| facts.cpu_percent())),
        facts.cpu_note()
    ));
    out.push_str(&format!(
        "ram         {}  {}\n",
        gauge(facts.host.map_or(0, |_| facts.ram_percent())),
        facts.ram_note()
    ));
    out.push_str(&format!(
        "disk        {}  {}\n",
        gauge(facts.host.map_or(0, |_| facts.disk_percent())),
        facts.disk_note()
    ));
    out.push_str(&format!("inbox       {} open\n", facts.inbox));
    out.push_str("```\n\n");

    out.push_str("## Projects\n\n");
    if snapshot.projects.is_empty() {
        out.push_str("_No projects registered._\n\n");
    } else {
        out.push_str("| Project | State | Running | Ready | Blocked |\n");
        out.push_str("| --- | --- | ---: | ---: | ---: |\n");
        for project in &snapshot.projects {
            let ready = project.jobs_by_state.get("ready").copied().unwrap_or(0);
            out.push_str(&format!(
                "| {} | {} | {} | {} | {} |\n",
                md_text(project.name.as_str()),
                project_state_name(project.state),
                project.running.len(),
                ready,
                project.blocked.len()
            ));
        }
        out.push('\n');
    }

    let brake = &snapshot.capacity.brake;
    if brake.active {
        out.push_str("## Brakes\n\n");
        if brake.reasons.is_empty() {
            out.push_str("- brake active\n");
        } else {
            for reason in &brake.reasons {
                out.push_str(&format!(
                    "- **{}** — {}\n",
                    brake_code_name(reason.code),
                    md_text(&reason.detail)
                ));
            }
        }
        out.push('\n');
    }

    let cluster = &snapshot.capacity.cluster;
    if cluster.online_worker_nodes > 0 {
        out.push_str(&format!("{}\n\n", crate::cmd::capacity::cluster_line(cluster)));
    }
    out
}

/// Renders the self-contained, zero-CDN Meadow HTML widget.
pub(crate) fn render_html(status: &FleetStatus, capacity: Option<&CapacityReport>) -> String {
    let facts = Facts::new(status, capacity);
    let snapshot = facts.snapshot();
    let home = escape_html(snapshot.home.as_str());
    let source = escape_html(&status.source);
    let fetched = escape_html(&status.fetched_at.as_rfc3339());
    let generated = escape_html(&snapshot.generated_at.as_rfc3339());
    let supervisor = &snapshot.supervisor;
    let (state_class, state_label) = if supervisor.running {
        ("ok", "running")
    } else if supervisor.paused {
        ("warn", "paused")
    } else {
        ("down", "stopped")
    };
    let freshness = if status.stale { "stale" } else { "fresh" };
    let uptime = facts
        .uptime_secs()
        .map(format_duration)
        .unwrap_or_else(|| "—".to_owned());
    let uptime_label = if supervisor.running { uptime } else { "—".to_owned() };
    let pid = supervisor.pid.map_or_else(|| "—".to_owned(), |pid| pid.to_string());
    let version = escape_html(&supervisor.version);
    let tick = supervisor.tick_secs;
    let last_tick = facts
        .last_tick_age_secs()
        .map(|age| format!("{} ago", format_duration(age)))
        .unwrap_or_else(|| "—".to_owned());
    let target = facts.target();
    let target_note = if target.is_override {
        "override"
    } else {
        facts.target_mode()
    };
    let advised = target
        .advised_workers
        .map_or_else(|| "—".to_owned(), |workers| workers.to_string());
    let brakes = &snapshot.capacity.brake;
    let cluster = &snapshot.capacity.cluster;

    let mut html = String::new();
    html.push_str(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <meta name=\"color-scheme\" content=\"light\">\n",
    );
    html.push_str(&format!("<title>StackSpirit fleet — {home}</title>\n"));
    html.push_str("<style>\n");
    html.push_str(STYLE);
    html.push_str("</style>\n</head>\n<body>\n<main class=\"shell\">\n");

    html.push_str("<header>\n");
    html.push_str("<div class=\"brand\"><span class=\"mark\" aria-hidden=\"true\">◆</span>StackSpirit</div>\n");
    html.push_str(&format!("<span class=\"pill {state_class}\">{state_label}</span>\n"));
    html.push_str("</header>\n");
    html.push_str("<h1>Fleet status</h1>\n");
    html.push_str(&format!(
        "<p class=\"meta\"><span>{home}</span> · source {source} · fetched {fetched} · <span class=\"freshness\">{freshness}</span></p>\n"
    ));

    html.push_str("<section class=\"stats\" aria-label=\"Fleet summary\">\n");
    html.push_str(&format!(
        "<article class=\"stat\"><h2>VPS</h2><p class=\"big\">{state_label}</p>\
         <p class=\"detail\">up {uptime_label} · tick {tick}s</p>\
         <p class=\"detail\">pid {pid} · v{version}</p></article>\n"
    ));
    html.push_str(&format!(
        "<article class=\"stat\"><h2>Workers</h2><p class=\"big\">{} / {}</p>\
         <p class=\"detail\">{} target · advised {advised}</p>\
         <p class=\"detail\">{} waiting · last tick {last_tick}</p></article>\n",
        facts.busy,
        facts.target,
        escape_html(target_note),
        facts.waiting
    ));
    html.push_str(&format!(
        "<article class=\"stat\"><h2>Active jobs</h2><p class=\"big\">{}</p>\
         <p class=\"detail\">{} ready · {} blocked</p>\
         <p class=\"detail\">{} project(s)</p></article>\n",
        facts.active,
        facts.ready,
        facts.blocked,
        snapshot.projects.len()
    ));
    html.push_str(&format!(
        "<article class=\"stat\"><h2>Inbox</h2><p class=\"big\">{}</p>\
         <p class=\"detail\">open questions and approvals</p></article>\n",
        facts.inbox
    ));
    html.push_str("</section>\n");

    html.push_str("<section class=\"bars\" aria-label=\"Capacity\">\n<h2 class=\"section-title\">Capacity</h2>\n");
    let cpu_value = if facts.host.is_some() {
        format!("{}%", facts.cpu_percent())
    } else {
        "—".to_owned()
    };
    let ram_value = if facts.host.is_some() {
        format!("{}%", facts.ram_percent())
    } else {
        "—".to_owned()
    };
    let disk_value = match facts.host {
        Some(host) => crate::cmd::capacity::format_bytes(host.disk_free_home_bytes),
        None => "—".to_owned(),
    };
    html.push_str(&bar_html(
        "CPU",
        "CPU load",
        facts.host.map_or(0, |_| facts.cpu_percent()),
        &cpu_value,
        &facts.cpu_note(),
        facts.host.is_some(),
    ));
    html.push_str(&bar_html(
        "RAM",
        "RAM in use",
        facts.host.map_or(0, |_| facts.ram_percent()),
        &ram_value,
        &facts.ram_note(),
        facts.host.is_some(),
    ));
    html.push_str(&bar_html(
        "Disk",
        "Disk headroom above the reserve",
        facts.host.map_or(0, |_| facts.disk_percent()),
        &disk_value,
        &facts.disk_note(),
        facts.host.is_some(),
    ));
    html.push_str(&bar_html(
        "Workers",
        "Worker slots in use",
        facts.worker_percent(),
        &format!("{} / {}", facts.busy, facts.target),
        &facts.workers_summary(),
        true,
    ));
    html.push_str("</section>\n");

    html.push_str("<section class=\"projects\">\n<h2 class=\"section-title\">Projects</h2>\n");
    if snapshot.projects.is_empty() {
        html.push_str("<p class=\"muted\">No projects registered.</p>\n");
    } else {
        html.push_str(
            "<table><thead><tr><th>Project</th><th>State</th><th class=\"num\">Running</th>\
             <th class=\"num\">Ready</th><th class=\"num\">Blocked</th></tr></thead><tbody>\n",
        );
        for project in &snapshot.projects {
            let ready = project.jobs_by_state.get("ready").copied().unwrap_or(0);
            html.push_str(&format!(
                "<tr><td>{}</td><td><span class=\"tag\">{}</span></td>\
                 <td class=\"num\">{}</td><td class=\"num\">{ready}</td><td class=\"num\">{}</td></tr>\n",
                escape_html(project.name.as_str()),
                project_state_name(project.state),
                project.running.len(),
                project.blocked.len()
            ));
        }
        html.push_str("</tbody></table>\n");
    }
    html.push_str("</section>\n");

    let active_jobs: Vec<(&str, &vowp_protocol::RunningJobCard)> = snapshot
        .projects
        .iter()
        .flat_map(|project| project.running.iter().map(move |run| (project.name.as_str(), run)))
        .collect();
    html.push_str("<section class=\"jobs\">\n<h2 class=\"section-title\">Active jobs</h2>\n");
    if active_jobs.is_empty() {
        html.push_str("<p class=\"muted\">No active jobs.</p>\n");
    } else {
        html.push_str("<ul>\n");
        for (project, run) in active_jobs.iter().take(12) {
            let executor = run.executor.as_deref().unwrap_or("—");
            html.push_str(&format!(
                "<li><span class=\"job\">{}</span> <span class=\"muted\">{} · {} · {} · {}</span></li>\n",
                escape_html(run.job.as_str()),
                escape_html(project),
                escape_html(&format!("{:?}", run.kind).to_lowercase()),
                escape_html(executor),
                format_duration(run.age_secs)
            ));
        }
        if active_jobs.len() > 12 {
            html.push_str(&format!(
                "<li class=\"muted\">…and {} more</li>\n",
                active_jobs.len() - 12
            ));
        }
        html.push_str("</ul>\n");
    }
    html.push_str("</section>\n");

    if brakes.active {
        html.push_str("<section class=\"brakes\">\n<h2 class=\"section-title\">Brakes</h2>\n<ul>\n");
        if brakes.reasons.is_empty() {
            html.push_str("<li>Brake active.</li>\n");
        } else {
            for reason in &brakes.reasons {
                html.push_str(&format!(
                    "<li><strong>{}</strong> — {}</li>\n",
                    brake_code_name(reason.code),
                    escape_html(&reason.detail)
                ));
            }
        }
        html.push_str("</ul>\n</section>\n");
    }

    html.push_str(&format!(
        "<footer>Zero-secret read-only view · snapshot {generated} · {} online worker node(s)</footer>\n",
        cluster.online_worker_nodes
    ));
    html.push_str("</main>\n</body>\n</html>\n");
    html
}

/// The widget's inline stylesheet. No remote font, image, script or stylesheet is used.
const STYLE: &str = r#"
:root {
  color-scheme: light;
  --meadow-light: #F5F7F0;
  --meadow-deep: #1E5B36;
  --meadow-primary: #43A85A;
  --meadow-dark: #0D120F;
  --meadow-sprout: #CDEDB0;
  --paper: #FFFFFF;
  --line: #DCE4D4;
  --muted: #55604F;
}
* { box-sizing: border-box; }
html, body { margin: 0; padding: 0; }
body {
  background: var(--meadow-light);
  color: var(--meadow-dark);
  font-family: ui-sans-serif, system-ui, -apple-system, "Segoe UI", Roboto, Helvetica, Arial, sans-serif;
  font-size: 14px;
  line-height: 1.45;
  padding: 16px;
}
.shell {
  max-width: 760px;
  margin: 0 auto;
  background: var(--paper);
  border: 1px solid var(--line);
  border-radius: 16px;
  padding: 20px;
  box-shadow: 0 1px 2px rgba(13, 18, 15, .05);
}
header { display: flex; align-items: center; justify-content: space-between; gap: 12px; }
.brand { display: flex; align-items: center; gap: 8px; color: var(--meadow-deep); font-weight: 700; letter-spacing: .02em; }
.mark { width: 24px; height: 24px; border-radius: 8px; background: var(--meadow-primary); color: var(--meadow-sprout); display: inline-flex; align-items: center; justify-content: center; font-size: 13px; }
.pill { border-radius: 999px; padding: 3px 10px; font-size: 12px; font-weight: 700; letter-spacing: .04em; }
.pill.ok { background: var(--meadow-sprout); color: var(--meadow-deep); }
.pill.warn { background: #F3E7B8; color: #5A4A00; }
.pill.down { background: #E9DDD8; color: #6B2B1F; }
h1 { font-size: 22px; line-height: 1.2; margin: 12px 0 2px; color: var(--meadow-deep); }
.meta { margin: 0 0 16px; color: var(--muted); font-size: 12px; overflow-wrap: anywhere; }
.freshness { color: var(--meadow-deep); font-weight: 600; }
.stats { display: grid; grid-template-columns: repeat(auto-fit, minmax(150px, 1fr)); gap: 10px; margin: 0 0 18px; }
.stat { border: 1px solid var(--line); border-radius: 12px; padding: 12px; background: #FBFCF8; }
.stat h2, .section-title { font-size: 11px; text-transform: uppercase; letter-spacing: .09em; color: var(--muted); margin: 0 0 6px; font-weight: 700; }
.big { font-size: 24px; line-height: 1.1; font-weight: 700; color: var(--meadow-deep); margin: 0 0 6px; }
.detail { margin: 0; color: var(--muted); font-size: 12px; }
.bars { border-top: 1px solid var(--line); padding-top: 14px; margin-bottom: 18px; }
.section-title { margin-bottom: 10px; }
.bar { margin-bottom: 12px; }
.bar-head { display: flex; justify-content: space-between; gap: 12px; font-size: 12px; font-weight: 600; margin-bottom: 4px; }
.bar-value { color: var(--meadow-deep); }
.track { height: 8px; border-radius: 999px; background: var(--meadow-light); border: 1px solid var(--line); overflow: hidden; }
.track span { display: block; height: 100%; background: var(--meadow-primary); border-radius: 999px; }
.track.empty span { background: var(--line); }
.bar-note { margin: 3px 0 0; color: var(--muted); font-size: 11px; }
.projects, .jobs, .brakes { border-top: 1px solid var(--line); padding-top: 14px; margin-bottom: 16px; }
table { width: 100%; border-collapse: collapse; font-size: 12px; }
th { text-align: left; color: var(--muted); font-weight: 700; padding: 4px 6px; border-bottom: 1px solid var(--line); }
td { padding: 6px; border-bottom: 1px solid var(--meadow-light); }
th.num, td.num { text-align: right; font-variant-numeric: tabular-nums; }
.tag { display: inline-block; border-radius: 999px; padding: 1px 8px; background: var(--meadow-light); color: var(--meadow-deep); font-size: 11px; font-weight: 600; }
ul { margin: 0; padding-left: 18px; }
li { margin: 3px 0; }
.job { font-weight: 700; color: var(--meadow-deep); overflow-wrap: anywhere; }
.muted { color: var(--muted); margin: 0; }
footer { border-top: 1px solid var(--line); padding-top: 10px; color: var(--muted); font-size: 11px; }
"#;

/// Returns one capacity bar block, escaping its dynamic value and note.
fn bar_html(label: &str, aria: &str, percent: u8, value: &str, note: &str, has_sample: bool) -> String {
    let value = escape_html(value);
    let note = escape_html(note);
    let empty = if has_sample { "" } else { " empty" };
    format!(
        "<div class=\"bar\"><div class=\"bar-head\"><span>{label}</span><span class=\"bar-value\">{value}</span></div>\
         <div class=\"track{empty}\" role=\"progressbar\" aria-label=\"{aria}\" aria-valuemin=\"0\" aria-valuemax=\"100\" aria-valuenow=\"{percent}\">\
         <span style=\"width:{percent}%\"></span></div><p class=\"bar-note\">{note}</p></div>\n"
    )
}

/// Renders an ASCII gauge of ten cells.
fn gauge(percent: u8) -> String {
    let filled = usize::from(percent.min(100)) * 10 / 100;
    let mut bar = String::with_capacity(12);
    bar.push('[');
    for index in 0..10 {
        bar.push(if index < filled { '#' } else { '-' });
    }
    bar.push(']');
    bar
}

/// Returns `part / whole` as a bounded whole percentage.
fn percent(part: u64, whole: u64) -> u8 {
    if whole == 0 {
        return 0;
    }
    u8::try_from((u128::from(part) * 100 / u128::from(whole)).min(100)).unwrap_or(100)
}

/// Formats a duration compactly as `2d 3h`, `2h 14m`, `3m 04s` or `12s`.
fn format_duration(secs: u64) -> String {
    if secs >= 86_400 {
        format!("{}d {}h", secs / 86_400, (secs % 86_400) / 3_600)
    } else if secs >= 3_600 {
        format!("{}h {:02}m", secs / 3_600, (secs % 3_600) / 60)
    } else if secs >= 60 {
        format!("{}m {:02}s", secs / 60, secs % 60)
    } else {
        format!("{secs}s")
    }
}

/// Escapes every HTML metacharacter in one dynamic field.
fn escape_html(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

/// Escapes Markdown metacharacters in one dynamic cell or label.
fn md_text(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\\' | '`' | '*' | '_' | '[' | ']' | '|' => {
                out.push('\\');
                out.push(ch);
            }
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\n' | '\r' => out.push(' '),
            _ => out.push(ch),
        }
    }
    out
}

/// Keeps a value safe inside a fenced code block: no backticks or control characters.
fn code_text(value: &str) -> String {
    value
        .chars()
        .map(|ch| if ch == '`' || ch.is_control() { ' ' } else { ch })
        .collect()
}

/// Returns the lowercase registry state name.
fn project_state_name(state: ProjectState) -> &'static str {
    match state {
        ProjectState::Active => "active",
        ProjectState::Paused => "paused",
        ProjectState::Archived => "archived",
    }
}

/// Returns the lowercase target mode name.
fn target_mode_name(mode: TargetMode) -> &'static str {
    match mode {
        TargetMode::Auto => "auto",
        TargetMode::Fixed => "fixed",
    }
}

/// Returns the machine-readable brake code.
fn brake_code_name(code: BrakeCode) -> &'static str {
    match code {
        BrakeCode::MemoryPressure => "memory_pressure",
        BrakeCode::SwapGrowth => "swap_growth",
        BrakeCode::DiskLow => "disk_low",
        BrakeCode::Provider => "provider",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_html_escapes_every_metacharacter() {
        assert_eq!(
            escape_html(r#"<script>alert("x&y")</script> 'quoted'"#),
            "&lt;script&gt;alert(&quot;x&amp;y&quot;)&lt;/script&gt; &#39;quoted&#39;"
        );
        assert_eq!(escape_html("plain"), "plain");
    }

    #[test]
    fn gauges_are_ascii_and_bounded() {
        assert_eq!(gauge(0), "[----------]");
        assert_eq!(gauge(50), "[#####-----]");
        assert_eq!(gauge(100), "[##########]");
        assert_eq!(gauge(200), "[##########]");
    }

    #[test]
    fn percent_is_bounded_and_safe_for_zero() {
        assert_eq!(percent(0, 0), 0);
        assert_eq!(percent(1, 0), 0);
        assert_eq!(percent(1, 3), 33);
        assert_eq!(percent(3, 3), 100);
        assert_eq!(percent(9, 3), 100);
    }

    #[test]
    fn markdown_escaping_cannot_start_a_table_cell_or_code_fence() {
        assert_eq!(md_text("a|b\nc`d"), "a\\|b c\\`d");
        assert_eq!(code_text("x`y\nz"), "x y z");
    }
}

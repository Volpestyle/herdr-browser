pub const DEFAULT_ICON: &str = "mods/native-browser/browser.svg";
const MARKDOWN_ICON: &str = "mods/native-browser/sites/markdown.svg";

const SITE_RULES: &[(&str, &str)] = &[
    // Specific subdomains that belong to distinct products under a parent domain
    ("mail.google.com", "mods/native-browser/sites/gmail.svg"),
    ("calendar.google.com", "mods/native-browser/sites/googlecalendar.svg"),
    ("drive.google.com", "mods/native-browser/sites/googledrive.svg"),
    ("meet.google.com", "mods/native-browser/sites/googlemeet.svg"),
    ("sheets.google.com", "mods/native-browser/sites/googlesheets.svg"),
    ("slides.google.com", "mods/native-browser/sites/googleslides.svg"),
    ("forms.google.com", "mods/native-browser/sites/googleforms.svg"),
    ("console.cloud.google.com", "mods/native-browser/sites/googlecloud.svg"),
    ("cloud.google.com", "mods/native-browser/sites/googlecloud.svg"),
    ("gemini.google.com", "mods/native-browser/sites/googlegemini.svg"),
    ("aistudio.google.com", "mods/native-browser/sites/googlegemini.svg"),
    ("colab.research.google.com", "mods/native-browser/sites/googlecolab.svg"),
    ("aws.amazon.com", "mods/native-browser/sites/amazonaws.svg"),
    ("awsapps.com", "mods/native-browser/sites/amazonaws.svg"),
    ("aws.dev", "mods/native-browser/sites/amazonaws.svg"),
    ("amazonaws.com", "mods/native-browser/sites/amazonaws.svg"),
    ("teams.microsoft.com", "mods/native-browser/sites/microsoftteams.svg"),
    ("teams.live.com", "mods/native-browser/sites/microsoftteams.svg"),
    ("teams.cloud.microsoft", "mods/native-browser/sites/microsoftteams.svg"),
    ("outlook.office.com", "mods/native-browser/sites/microsoftoutlook.svg"),
    ("outlook.office365.com", "mods/native-browser/sites/microsoftoutlook.svg"),
    ("outlook.live.com", "mods/native-browser/sites/microsoftoutlook.svg"),
    ("outlook.cloud.microsoft", "mods/native-browser/sites/microsoftoutlook.svg"),
    ("sharepoint.com", "mods/native-browser/sites/microsoftsharepoint.svg"),
    ("azure.microsoft.com", "mods/native-browser/sites/microsoftazure.svg"),
    ("azure.com", "mods/native-browser/sites/microsoftazure.svg"),
    ("visualstudio.com", "mods/native-browser/sites/microsoftazure.svg"),
    ("developer.mozilla.org", "mods/native-browser/sites/mdnwebdocs.svg"),
    ("news.ycombinator.com", "mods/native-browser/sites/ycombinator.svg"),
    ("ycombinator.com", "mods/native-browser/sites/ycombinator.svg"),
    ("mail.proton.me", "mods/native-browser/sites/protonmail.svg"),
    ("proton.me", "mods/native-browser/sites/protonmail.svg"),
    ("protonmail.com", "mods/native-browser/sites/protonmail.svg"),
    // Project management & productivity
    ("linear.app", "mods/native-browser/sites/linear.svg"),
    ("trello.com", "mods/native-browser/sites/trello.svg"),
    ("asana.com", "mods/native-browser/sites/asana.svg"),
    ("notion.so", "mods/native-browser/sites/notion.svg"),
    ("notion.site", "mods/native-browser/sites/notion.svg"),
    ("notion.com", "mods/native-browser/sites/notion.svg"),
    ("shortcut.com", "mods/native-browser/sites/shortcut.svg"),
    ("clickup.com", "mods/native-browser/sites/clickup.svg"),
    ("airtable.com", "mods/native-browser/sites/airtable.svg"),
    ("basecamp.com", "mods/native-browser/sites/basecamp.svg"),
    ("coda.io", "mods/native-browser/sites/coda.svg"),
    ("miro.com", "mods/native-browser/sites/miro.svg"),
    ("obsidian.md", "mods/native-browser/sites/obsidian.svg"),
    ("loom.com", "mods/native-browser/sites/loom.svg"),
    ("atlassian.com", "mods/native-browser/sites/atlassian.svg"),
    // Code hosting, CI/CD & package registries
    ("github.com", "mods/native-browser/sites/github.svg"),
    ("github.dev", "mods/native-browser/sites/github.svg"),
    ("github.blog", "mods/native-browser/sites/github.svg"),
    ("gitlab.com", "mods/native-browser/sites/gitlab.svg"),
    ("bitbucket.org", "mods/native-browser/sites/bitbucket.svg"),
    ("codeberg.org", "mods/native-browser/sites/codeberg.svg"),
    ("buildkite.com", "mods/native-browser/sites/buildkite.svg"),
    ("circleci.com", "mods/native-browser/sites/circleci.svg"),
    ("jenkins.io", "mods/native-browser/sites/jenkins.svg"),
    ("argoproj.io", "mods/native-browser/sites/argo.svg"),
    ("npmjs.com", "mods/native-browser/sites/npm.svg"),
    ("pypi.org", "mods/native-browser/sites/pypi.svg"),
    ("crates.io", "mods/native-browser/sites/rust.svg"),
    ("docs.rs", "mods/native-browser/sites/rust.svg"),
    ("rust-lang.org", "mods/native-browser/sites/rust.svg"),
    ("swagger.io", "mods/native-browser/sites/swagger.svg"),
    // Observability, monitoring & analytics
    ("datadoghq.com", "mods/native-browser/sites/datadog.svg"),
    ("datadoghq.eu", "mods/native-browser/sites/datadog.svg"),
    ("ddog-gov.com", "mods/native-browser/sites/datadog.svg"),
    ("datadog.com", "mods/native-browser/sites/datadog.svg"),
    ("sentry.io", "mods/native-browser/sites/sentry.svg"),
    ("grafana.com", "mods/native-browser/sites/grafana.svg"),
    ("grafana.net", "mods/native-browser/sites/grafana.svg"),
    ("pagerduty.com", "mods/native-browser/sites/pagerduty.svg"),
    ("newrelic.com", "mods/native-browser/sites/newrelic.svg"),
    ("posthog.com", "mods/native-browser/sites/posthog.svg"),
    ("elastic.co", "mods/native-browser/sites/elastic.svg"),
    ("elastic-cloud.com", "mods/native-browser/sites/elastic.svg"),
    ("prometheus.io", "mods/native-browser/sites/prometheus.svg"),
    ("statuspage.io", "mods/native-browser/sites/statuspage.svg"),
    ("mixpanel.com", "mods/native-browser/sites/mixpanel.svg"),
    // Cloud, infra, DBs, auth & dev platforms
    ("cloudflare.com", "mods/native-browser/sites/cloudflare.svg"),
    ("cloudflarestatus.com", "mods/native-browser/sites/cloudflare.svg"),
    ("vercel.com", "mods/native-browser/sites/vercel.svg"),
    ("netlify.com", "mods/native-browser/sites/netlify.svg"),
    ("fly.io", "mods/native-browser/sites/flydotio.svg"),
    ("railway.app", "mods/native-browser/sites/railway.svg"),
    ("railway.com", "mods/native-browser/sites/railway.svg"),
    ("render.com", "mods/native-browser/sites/render.svg"),
    ("heroku.com", "mods/native-browser/sites/heroku.svg"),
    ("digitalocean.com", "mods/native-browser/sites/digitalocean.svg"),
    ("supabase.com", "mods/native-browser/sites/supabase.svg"),
    ("supabase.co", "mods/native-browser/sites/supabase.svg"),
    ("neon.tech", "mods/native-browser/sites/neon.svg"),
    ("planetscale.com", "mods/native-browser/sites/planetscale.svg"),
    ("upstash.com", "mods/native-browser/sites/upstash.svg"),
    ("turso.tech", "mods/native-browser/sites/turso.svg"),
    ("terraform.io", "mods/native-browser/sites/terraform.svg"),
    ("hashicorp.com", "mods/native-browser/sites/hashicorp.svg"),
    ("docker.com", "mods/native-browser/sites/docker.svg"),
    ("tailscale.com", "mods/native-browser/sites/tailscale.svg"),
    ("stripe.com", "mods/native-browser/sites/stripe.svg"),
    ("twilio.com", "mods/native-browser/sites/twilio.svg"),
    ("okta.com", "mods/native-browser/sites/okta.svg"),
    ("oktapreview.com", "mods/native-browser/sites/okta.svg"),
    ("okta-emea.com", "mods/native-browser/sites/okta.svg"),
    ("1password.com", "mods/native-browser/sites/1password.svg"),
    ("auth0.com", "mods/native-browser/sites/auth0.svg"),
    ("clerk.com", "mods/native-browser/sites/clerk.svg"),
    ("postman.com", "mods/native-browser/sites/postman.svg"),
    ("postman.co", "mods/native-browser/sites/postman.svg"),
    ("resend.com", "mods/native-browser/sites/resend.svg"),
    ("algolia.com", "mods/native-browser/sites/algolia.svg"),
    ("snowflake.com", "mods/native-browser/sites/snowflake.svg"),
    ("snowflakecomputing.com", "mods/native-browser/sites/snowflake.svg"),
    ("databricks.com", "mods/native-browser/sites/databricks.svg"),
    ("metabase.com", "mods/native-browser/sites/metabase.svg"),
    ("retool.com", "mods/native-browser/sites/retool.svg"),
    ("shopify.com", "mods/native-browser/sites/shopify.svg"),
    // Communication, email, CRM, HR, finance & storage
    ("slack.com", "mods/native-browser/sites/slack.svg"),
    ("discord.com", "mods/native-browser/sites/discord.svg"),
    ("discordapp.com", "mods/native-browser/sites/discord.svg"),
    ("zoom.us", "mods/native-browser/sites/zoom.svg"),
    ("zoom.com", "mods/native-browser/sites/zoom.svg"),
    ("calendly.com", "mods/native-browser/sites/calendly.svg"),
    ("hubspot.com", "mods/native-browser/sites/hubspot.svg"),
    ("salesforce.com", "mods/native-browser/sites/salesforce.svg"),
    ("force.com", "mods/native-browser/sites/salesforce.svg"),
    ("zendesk.com", "mods/native-browser/sites/zendesk.svg"),
    ("intercom.com", "mods/native-browser/sites/intercom.svg"),
    ("docusign.com", "mods/native-browser/sites/docusign.svg"),
    ("docusign.net", "mods/native-browser/sites/docusign.svg"),
    ("dropbox.com", "mods/native-browser/sites/dropbox.svg"),
    ("box.com", "mods/native-browser/sites/box.svg"),
    ("gusto.com", "mods/native-browser/sites/gusto.svg"),
    ("brex.com", "mods/native-browser/sites/brex.svg"),
    ("whatsapp.com", "mods/native-browser/sites/whatsapp.svg"),
    ("telegram.org", "mods/native-browser/sites/telegram.svg"),
    ("t.me", "mods/native-browser/sites/telegram.svg"),
    // Design, whiteboarding, docs & sandboxes
    ("figma.com", "mods/native-browser/sites/figma.svg"),
    ("excalidraw.com", "mods/native-browser/sites/excalidraw.svg"),
    ("tldraw.com", "mods/native-browser/sites/tldraw.svg"),
    ("canva.com", "mods/native-browser/sites/canva.svg"),
    ("framer.com", "mods/native-browser/sites/framer.svg"),
    ("storybook.js.org", "mods/native-browser/sites/storybook.svg"),
    ("chromatic.com", "mods/native-browser/sites/chromatic.svg"),
    ("webflow.com", "mods/native-browser/sites/webflow.svg"),
    ("lucid.app", "mods/native-browser/sites/lucid.svg"),
    ("lucidchart.com", "mods/native-browser/sites/lucid.svg"),
    ("diagrams.net", "mods/native-browser/sites/diagramsdotnet.svg"),
    ("draw.io", "mods/native-browser/sites/diagramsdotnet.svg"),
    ("mermaid.live", "mods/native-browser/sites/mermaid.svg"),
    ("mermaid.js.org", "mods/native-browser/sites/mermaid.svg"),
    ("mermaidchart.com", "mods/native-browser/sites/mermaid.svg"),
    ("gitbook.com", "mods/native-browser/sites/gitbook.svg"),
    ("gitbook.io", "mods/native-browser/sites/gitbook.svg"),
    ("replit.com", "mods/native-browser/sites/replit.svg"),
    ("codesandbox.io", "mods/native-browser/sites/codesandbox.svg"),
    ("stackblitz.com", "mods/native-browser/sites/stackblitz.svg"),
    // AI & LLM tools
    ("claude.ai", "mods/native-browser/sites/claude.svg"),
    ("claude.com", "mods/native-browser/sites/claude.svg"),
    ("anthropic.com", "mods/native-browser/sites/anthropic.svg"),
    ("chatgpt.com", "mods/native-browser/sites/openai.svg"),
    ("openai.com", "mods/native-browser/sites/openai.svg"),
    ("perplexity.ai", "mods/native-browser/sites/perplexity.svg"),
    ("huggingface.co", "mods/native-browser/sites/huggingface.svg"),
    ("cursor.com", "mods/native-browser/sites/cursor.svg"),
    ("cursor.sh", "mods/native-browser/sites/cursor.svg"),
    ("v0.dev", "mods/native-browser/sites/v0.svg"),
    ("v0.app", "mods/native-browser/sites/v0.svg"),
    ("deepseek.com", "mods/native-browser/sites/deepseek.svg"),
    ("mistral.ai", "mods/native-browser/sites/mistralai.svg"),
    // Search, dev reference, social & media
    ("duckduckgo.com", "mods/native-browser/sites/duckduckgo.svg"),
    ("youtube.com", "mods/native-browser/sites/youtube.svg"),
    ("youtu.be", "mods/native-browser/sites/youtube.svg"),
    ("x.com", "mods/native-browser/sites/x.svg"),
    ("twitter.com", "mods/native-browser/sites/x.svg"),
    ("reddit.com", "mods/native-browser/sites/reddit.svg"),
    ("stackoverflow.com", "mods/native-browser/sites/stackoverflow.svg"),
    ("stackexchange.com", "mods/native-browser/sites/stackoverflow.svg"),
    ("serverfault.com", "mods/native-browser/sites/stackoverflow.svg"),
    ("superuser.com", "mods/native-browser/sites/stackoverflow.svg"),
    ("wikipedia.org", "mods/native-browser/sites/wikipedia.svg"),
    ("linkedin.com", "mods/native-browser/sites/linkedin.svg"),
    ("bsky.app", "mods/native-browser/sites/bluesky.svg"),
    ("spotify.com", "mods/native-browser/sites/spotify.svg"),
    ("twitch.tv", "mods/native-browser/sites/twitch.svg"),
    ("substack.com", "mods/native-browser/sites/substack.svg"),
    ("medium.com", "mods/native-browser/sites/medium.svg"),
    ("icloud.com", "mods/native-browser/sites/icloud.svg"),
    ("apple.com", "mods/native-browser/sites/apple.svg"),
    ("instagram.com", "mods/native-browser/sites/instagram.svg"),
    ("threads.net", "mods/native-browser/sites/threads.svg"),
    ("threads.com", "mods/native-browser/sites/threads.svg"),
    ("mastodon.social", "mods/native-browser/sites/mastodon.svg"),
    ("dribbble.com", "mods/native-browser/sites/dribbble.svg"),
    ("unsplash.com", "mods/native-browser/sites/unsplash.svg"),
    // Broad parent-domain fallbacks (checked after product-specific subdomains)
    ("microsoft.com", "mods/native-browser/sites/microsoft.svg"),
    ("microsoftonline.com", "mods/native-browser/sites/microsoft.svg"),
    ("office.com", "mods/native-browser/sites/microsoft.svg"),
    ("office365.com", "mods/native-browser/sites/microsoft.svg"),
    ("live.com", "mods/native-browser/sites/microsoft.svg"),
    ("cloud.microsoft", "mods/native-browser/sites/microsoft.svg"),
    ("amazon.com", "mods/native-browser/sites/amazon.svg"),
    ("amazon.co.uk", "mods/native-browser/sites/amazon.svg"),
    ("amazon.ca", "mods/native-browser/sites/amazon.svg"),
    ("amazon.de", "mods/native-browser/sites/amazon.svg"),
    ("amazon.co.jp", "mods/native-browser/sites/amazon.svg"),
    ("google.com", "mods/native-browser/sites/google.svg"),
    ("google.co.uk", "mods/native-browser/sites/google.svg"),
    ("google.ca", "mods/native-browser/sites/google.svg"),
    ("google.de", "mods/native-browser/sites/google.svg"),
    ("google.fr", "mods/native-browser/sites/google.svg"),
    ("google.co.jp", "mods/native-browser/sites/google.svg"),
    ("google.com.au", "mods/native-browser/sites/google.svg"),
    ("google.es", "mods/native-browser/sites/google.svg"),
    ("google.it", "mods/native-browser/sites/google.svg"),
    ("google.nl", "mods/native-browser/sites/google.svg"),
    ("google.co.in", "mods/native-browser/sites/google.svg"),
];

const SELF_HOSTED_PREFIXES: &[(&str, &str)] = &[
    ("gitlab.", "mods/native-browser/sites/gitlab.svg"),
    ("jira.", "mods/native-browser/sites/jira.svg"),
    ("confluence.", "mods/native-browser/sites/confluence.svg"),
    ("bitbucket.", "mods/native-browser/sites/bitbucket.svg"),
    ("grafana.", "mods/native-browser/sites/grafana.svg"),
    ("jenkins.", "mods/native-browser/sites/jenkins.svg"),
    ("kibana.", "mods/native-browser/sites/kibana.svg"),
    ("argocd.", "mods/native-browser/sites/argo.svg"),
    ("argo-cd.", "mods/native-browser/sites/argo.svg"),
    ("prometheus.", "mods/native-browser/sites/prometheus.svg"),
    ("sentry.", "mods/native-browser/sites/sentry.svg"),
    ("metabase.", "mods/native-browser/sites/metabase.svg"),
    ("storybook.", "mods/native-browser/sites/storybook.svg"),
];

fn host_matches(host: &str, domain: &str) -> bool {
    host == domain
        || (host.len() > domain.len()
            && host.ends_with(domain)
            && host.as_bytes()[host.len() - domain.len() - 1] == b'.')
}

pub fn site_icon(url: &str) -> &'static str {
    let Ok(parsed) = url::Url::parse(url.trim()) else {
        return DEFAULT_ICON;
    };
    if !matches!(parsed.scheme(), "http" | "https") {
        return DEFAULT_ICON;
    }
    let Some(raw_host) = parsed.host_str() else {
        return DEFAULT_ICON;
    };
    let lowered = raw_host.trim_end_matches('.').to_ascii_lowercase();
    let host = lowered.strip_prefix("www.").unwrap_or(&lowered);
    let path = parsed.path();

    if host_matches(host, "atlassian.net") {
        if path == "/wiki" || path.starts_with("/wiki/") {
            return "mods/native-browser/sites/confluence.svg";
        }
        if path == "/jira"
            || path.starts_with("/jira/")
            || path.starts_with("/browse/")
            || path.starts_with("/projects/")
            || path.starts_with("/servicedesk")
            || path.starts_with("/secure/")
        {
            return "mods/native-browser/sites/jira.svg";
        }
        return "mods/native-browser/sites/atlassian.svg";
    }

    if host == "docs.google.com" {
        if path.starts_with("/spreadsheets") {
            return "mods/native-browser/sites/googlesheets.svg";
        }
        if path.starts_with("/presentation") {
            return "mods/native-browser/sites/googleslides.svg";
        }
        if path.starts_with("/forms") {
            return "mods/native-browser/sites/googleforms.svg";
        }
        return "mods/native-browser/sites/googledocs.svg";
    }

    for &(domain, icon) in SITE_RULES {
        if host_matches(host, domain) {
            return icon;
        }
    }

    for &(prefix, icon) in SELF_HOSTED_PREFIXES {
        if host.starts_with(prefix) {
            return icon;
        }
    }

    DEFAULT_ICON
}

pub fn pane_icon(url: &str, is_markdown: bool) -> &'static str {
    if is_markdown
        && let Ok(parsed) = url::Url::parse(url.trim())
        && matches!(parsed.host_str(), Some("127.0.0.1" | "localhost" | "::1"))
    {
        return MARKDOWN_ICON;
    }
    site_icon(url)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn matches_common_workplace_tools_and_paths() {
        let cases = [
            ("https://linear.app/vibes/issue/ORC-123", "mods/native-browser/sites/linear.svg"),
            ("https://www.linear.app/", "mods/native-browser/sites/linear.svg"),
            ("https://gitlab.com/vibes/repos/studio/-/merge_requests/1", "mods/native-browser/sites/gitlab.svg"),
            ("https://gitlab.internal.company.com/team/repo", "mods/native-browser/sites/gitlab.svg"),
            ("https://github.com/Volpestyle/herdr-browser", "mods/native-browser/sites/github.svg"),
            ("https://gist.github.com/user/123", "mods/native-browser/sites/github.svg"),
            ("https://app.datadoghq.com/logs", "mods/native-browser/sites/datadog.svg"),
            ("https://app.slack.com/client/T123/C456", "mods/native-browser/sites/slack.svg"),
            ("https://acme.atlassian.net/browse/ENG-42", "mods/native-browser/sites/jira.svg"),
            ("https://acme.atlassian.net/jira/software/projects/ENG", "mods/native-browser/sites/jira.svg"),
            ("https://acme.atlassian.net/wiki/spaces/ENG/pages/1", "mods/native-browser/sites/confluence.svg"),
            ("https://acme.atlassian.net/admin", "mods/native-browser/sites/atlassian.svg"),
            ("https://docs.google.com/document/d/abc/edit", "mods/native-browser/sites/googledocs.svg"),
            ("https://docs.google.com/spreadsheets/d/abc/edit", "mods/native-browser/sites/googlesheets.svg"),
            ("https://docs.google.com/presentation/d/abc/edit", "mods/native-browser/sites/googleslides.svg"),
            ("https://docs.google.com/forms/d/abc/viewform", "mods/native-browser/sites/googleforms.svg"),
            ("https://mail.google.com/mail/u/0/#inbox", "mods/native-browser/sites/gmail.svg"),
            ("https://calendar.google.com/calendar/u/0/r", "mods/native-browser/sites/googlecalendar.svg"),
            ("https://meet.google.com/abc-defg-hij", "mods/native-browser/sites/googlemeet.svg"),
            ("https://drive.google.com/drive/my-drive", "mods/native-browser/sites/googledrive.svg"),
            ("https://www.google.com/search?q=rust", "mods/native-browser/sites/google.svg"),
            ("https://us-east-1.console.aws.amazon.com/lambda/home", "mods/native-browser/sites/amazonaws.svg"),
            ("https://d-123.awsapps.com/start", "mods/native-browser/sites/amazonaws.svg"),
            ("https://www.amazon.com/dp/B000", "mods/native-browser/sites/amazon.svg"),
            ("https://teams.microsoft.com/v2/", "mods/native-browser/sites/microsoftteams.svg"),
            ("https://outlook.office.com/mail/", "mods/native-browser/sites/microsoftoutlook.svg"),
            ("https://acme.sharepoint.com/sites/eng", "mods/native-browser/sites/microsoftsharepoint.svg"),
            ("https://portal.azure.com/#home", "mods/native-browser/sites/microsoftazure.svg"),
            ("https://www.figma.com/design/abc/Spec", "mods/native-browser/sites/figma.svg"),
            ("https://www.notion.so/Workspace-123", "mods/native-browser/sites/notion.svg"),
            ("https://claude.ai/chat/123", "mods/native-browser/sites/claude.svg"),
            ("https://console.anthropic.com/settings", "mods/native-browser/sites/anthropic.svg"),
            ("https://chatgpt.com/c/123", "mods/native-browser/sites/openai.svg"),
            ("https://acme.okta.com/app/UserHome", "mods/native-browser/sites/okta.svg"),
            ("https://grafana.prod.internal/d/abc", "mods/native-browser/sites/grafana.svg"),
            ("https://kibana.corp.internal/app/discover", "mods/native-browser/sites/kibana.svg"),
            ("https://argocd.corp.internal/applications", "mods/native-browser/sites/argo.svg"),
        ];
        for (url, expected) in cases {
            assert_eq!(site_icon(url), expected, "url: {url}");
        }
    }

    #[test]
    fn rejects_lookalikes_and_falls_back_to_globe() {
        for url in [
            "",
            "about:blank",
            "not-a-url",
            "file:///tmp/index.html",
            "http://localhost:3000/app",
            "http://127.0.0.1:8080/",
            "https://notlinear.app/",
            "https://evilgithub.com/",
            "https://my-gitlab.example.com/",
            "https://example.com/",
        ] {
            assert_eq!(site_icon(url), DEFAULT_ICON, "url: {url}");
        }
    }

    #[test]
    fn markdown_preview_uses_markdown_icon_until_navigated_away() {
        assert_eq!(
            pane_icon("http://127.0.0.1:54321/token/README.md", true),
            MARKDOWN_ICON
        );
        assert_eq!(
            pane_icon("https://linear.app/vibes", true),
            "mods/native-browser/sites/linear.svg"
        );
        assert_eq!(
            pane_icon("http://127.0.0.1:54321/token/README.md", false),
            DEFAULT_ICON
        );
    }

    #[test]
    fn every_referenced_site_icon_is_shipped_and_none_are_orphaned() {
        let shipped: BTreeSet<String> = crate::mods::DEFAULT_MODS
            .iter()
            .filter_map(|(path, _)| path.starts_with("native-browser/sites/").then(|| format!("mods/{path}")))
            .collect();
        let mut referenced: BTreeSet<&'static str> = BTreeSet::new();
        referenced.insert(MARKDOWN_ICON);
        referenced.insert("mods/native-browser/sites/confluence.svg");
        referenced.insert("mods/native-browser/sites/jira.svg");
        referenced.insert("mods/native-browser/sites/atlassian.svg");
        referenced.insert("mods/native-browser/sites/googlesheets.svg");
        referenced.insert("mods/native-browser/sites/googleslides.svg");
        referenced.insert("mods/native-browser/sites/googleforms.svg");
        referenced.insert("mods/native-browser/sites/googledocs.svg");
        for &(_, icon) in SITE_RULES {
            referenced.insert(icon);
        }
        for &(_, icon) in SELF_HOSTED_PREFIXES {
            referenced.insert(icon);
        }
        for icon in &referenced {
            assert!(shipped.contains(*icon), "icon referenced in rules is not shipped: {icon}");
        }
        for icon in &shipped {
            assert!(referenced.contains(icon.as_str()), "shipped site icon is never referenced: {icon}");
        }
    }
}

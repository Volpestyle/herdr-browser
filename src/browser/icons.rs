pub const DEFAULT_ICON: &str = "mods/native-browser/browser.svg";
const MARKDOWN_ICON: &str = "mods/native-browser/sites/markdown.svg";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SiteIcon {
    pub path: &'static str,
    pub tint: bool,
    pub color: Option<&'static str>,
}

const DEFAULT: SiteIcon = SiteIcon {
    path: DEFAULT_ICON,
    tint: true,
    color: None,
};

const ICON_STYLES: &[(&str, bool, Option<&str>)] = &[
    ("mods/native-browser/sites/1password.svg", true, Some("#3B82F6")),
    ("mods/native-browser/sites/airtable.svg", false, None),
    ("mods/native-browser/sites/algolia.svg", true, Some("#295CFF")),
    ("mods/native-browser/sites/amazon.svg", true, Some("#FFA929")),
    ("mods/native-browser/sites/amazonaws.svg", true, Some("#FF9900")),
    ("mods/native-browser/sites/anthropic.svg", true, None),
    ("mods/native-browser/sites/apple.svg", true, None),
    ("mods/native-browser/sites/argo.svg", false, None),
    ("mods/native-browser/sites/asana.svg", true, Some("#F06A6A")),
    ("mods/native-browser/sites/atlassian.svg", false, None),
    ("mods/native-browser/sites/auth0.svg", true, Some("#ED663B")),
    ("mods/native-browser/sites/basecamp.svg", true, Some("#59C173")),
    ("mods/native-browser/sites/bitbucket.svg", false, None),
    ("mods/native-browser/sites/bluesky.svg", true, Some("#2A92FE")),
    ("mods/native-browser/sites/box.svg", true, Some("#298AFF")),
    ("mods/native-browser/sites/brex.svg", true, Some("#F46A25")),
    ("mods/native-browser/sites/buildkite.svg", true, Some("#3CECA3")),
    ("mods/native-browser/sites/calendly.svg", true, Some("#2983FF")),
    ("mods/native-browser/sites/canva.svg", true, Some("#29F7FF")),
    ("mods/native-browser/sites/chromatic.svg", false, None),
    ("mods/native-browser/sites/circleci.svg", true, Some("#78B844")),
    ("mods/native-browser/sites/claude.svg", true, Some("#D97757")),
    ("mods/native-browser/sites/clerk.svg", true, Some("#6C47FF")),
    ("mods/native-browser/sites/clickup.svg", false, None),
    ("mods/native-browser/sites/cloudflare.svg", false, None),
    ("mods/native-browser/sites/coda.svg", true, Some("#F46A54")),
    ("mods/native-browser/sites/codeberg.svg", true, Some("#2185D0")),
    ("mods/native-browser/sites/codesandbox.svg", true, None),
    ("mods/native-browser/sites/confluence.svg", false, None),
    ("mods/native-browser/sites/cursor.svg", true, None),
    ("mods/native-browser/sites/databricks.svg", true, Some("#FF3621")),
    ("mods/native-browser/sites/datadog.svg", true, Some("#9B6BFF")),
    ("mods/native-browser/sites/deepseek.svg", true, Some("#5786FE")),
    ("mods/native-browser/sites/diagramsdotnet.svg", true, Some("#FB9F2D")),
    ("mods/native-browser/sites/digitalocean.svg", true, Some("#2994FF")),
    ("mods/native-browser/sites/discord.svg", true, Some("#5865F2")),
    ("mods/native-browser/sites/docker.svg", true, Some("#39A0EF")),
    ("mods/native-browser/sites/docusign.svg", true, Some("#FFCE00")),
    ("mods/native-browser/sites/dribbble.svg", false, None),
    ("mods/native-browser/sites/dropbox.svg", true, Some("#297AFF")),
    ("mods/native-browser/sites/duckduckgo.svg", false, None),
    ("mods/native-browser/sites/elastic.svg", true, Some("#29CAFF")),
    ("mods/native-browser/sites/excalidraw.svg", true, Some("#6965DB")),
    ("mods/native-browser/sites/figma.svg", false, None),
    ("mods/native-browser/sites/flydotio.svg", false, None),
    ("mods/native-browser/sites/framer.svg", true, Some("#2970FF")),
    ("mods/native-browser/sites/gitbook.svg", true, Some("#B2E3EE")),
    ("mods/native-browser/sites/github.svg", true, None),
    ("mods/native-browser/sites/gitlab.svg", false, None),
    ("mods/native-browser/sites/gmail.svg", false, None),
    ("mods/native-browser/sites/google.svg", false, None),
    ("mods/native-browser/sites/googlecalendar.svg", false, None),
    ("mods/native-browser/sites/googlecloud.svg", false, None),
    ("mods/native-browser/sites/googlecolab.svg", true, Some("#F9AB00")),
    ("mods/native-browser/sites/googledocs.svg", true, Some("#4285F4")),
    ("mods/native-browser/sites/googledrive.svg", false, None),
    ("mods/native-browser/sites/googleforms.svg", true, Some("#9B6BFF")),
    ("mods/native-browser/sites/googlegemini.svg", true, Some("#8E75B2")),
    ("mods/native-browser/sites/googlemeet.svg", false, None),
    ("mods/native-browser/sites/googlesheets.svg", true, Some("#34A853")),
    ("mods/native-browser/sites/googleslides.svg", true, Some("#FBBC04")),
    ("mods/native-browser/sites/grafana.svg", false, None),
    ("mods/native-browser/sites/gusto.svg", true, Some("#F45D48")),
    ("mods/native-browser/sites/hashicorp.svg", true, Some("#7B42BC")),
    ("mods/native-browser/sites/heroku.svg", true, Some("#9E63FF")),
    ("mods/native-browser/sites/hubspot.svg", true, Some("#FF7A59")),
    ("mods/native-browser/sites/huggingface.svg", false, None),
    ("mods/native-browser/sites/icloud.svg", true, Some("#3693F3")),
    ("mods/native-browser/sites/instagram.svg", true, Some("#FF2981")),
    ("mods/native-browser/sites/intercom.svg", true, Some("#286EFA")),
    ("mods/native-browser/sites/jenkins.svg", true, Some("#D75F50")),
    ("mods/native-browser/sites/jira.svg", false, None),
    ("mods/native-browser/sites/kibana.svg", false, None),
    ("mods/native-browser/sites/linear.svg", true, Some("#5E6AD2")),
    ("mods/native-browser/sites/linkedin.svg", true, Some("#2D8CFF")),
    ("mods/native-browser/sites/loom.svg", true, Some("#625DF5")),
    ("mods/native-browser/sites/lucid.svg", true, Some("#F97B38")),
    ("mods/native-browser/sites/markdown.svg", true, None),
    ("mods/native-browser/sites/mastodon.svg", false, None),
    ("mods/native-browser/sites/mdnwebdocs.svg", true, None),
    ("mods/native-browser/sites/medium.svg", true, None),
    ("mods/native-browser/sites/mermaid.svg", true, Some("#FF3670")),
    ("mods/native-browser/sites/metabase.svg", true, Some("#509EE3")),
    ("mods/native-browser/sites/microsoft.svg", false, None),
    ("mods/native-browser/sites/microsoftazure.svg", false, None),
    ("mods/native-browser/sites/microsoftoutlook.svg", true, Some("#28A8EA")),
    ("mods/native-browser/sites/microsoftsharepoint.svg", true, Some("#038387")),
    ("mods/native-browser/sites/microsoftteams.svg", false, None),
    ("mods/native-browser/sites/miro.svg", true, Some("#FFD02F")),
    ("mods/native-browser/sites/mistralai.svg", false, None),
    ("mods/native-browser/sites/mixpanel.svg", true, Some("#7856FF")),
    ("mods/native-browser/sites/neon.svg", false, None),
    ("mods/native-browser/sites/netlify.svg", true, Some("#00C7B7")),
    ("mods/native-browser/sites/newrelic.svg", true, Some("#3DEB95")),
    ("mods/native-browser/sites/notion.svg", true, None),
    ("mods/native-browser/sites/npm.svg", false, None),
    ("mods/native-browser/sites/obsidian.svg", false, None),
    ("mods/native-browser/sites/okta.svg", true, Some("#2AA3E8")),
    ("mods/native-browser/sites/openai.svg", true, None),
    ("mods/native-browser/sites/pagerduty.svg", true, Some("#30F86C")),
    ("mods/native-browser/sites/perplexity.svg", true, Some("#45D0E3")),
    ("mods/native-browser/sites/planetscale.svg", true, None),
    ("mods/native-browser/sites/posthog.svg", false, None),
    ("mods/native-browser/sites/postman.svg", false, None),
    ("mods/native-browser/sites/prometheus.svg", true, Some("#E8623F")),
    ("mods/native-browser/sites/protonmail.svg", true, Some("#6D4AFF")),
    ("mods/native-browser/sites/pypi.svg", false, None),
    ("mods/native-browser/sites/railway.svg", true, None),
    ("mods/native-browser/sites/reddit.svg", false, None),
    ("mods/native-browser/sites/render.svg", true, None),
    ("mods/native-browser/sites/replit.svg", true, Some("#F97D2F")),
    ("mods/native-browser/sites/resend.svg", true, None),
    ("mods/native-browser/sites/retool.svg", true, Some("#F2633A")),
    ("mods/native-browser/sites/rust.svg", true, Some("#F74C00")),
    ("mods/native-browser/sites/salesforce.svg", false, None),
    ("mods/native-browser/sites/sentry.svg", true, Some("#8C7AE6")),
    ("mods/native-browser/sites/shopify.svg", false, None),
    ("mods/native-browser/sites/shortcut.svg", true, Some("#494BCB")),
    ("mods/native-browser/sites/slack.svg", false, None),
    ("mods/native-browser/sites/snowflake.svg", true, Some("#3EBCEA")),
    ("mods/native-browser/sites/spotify.svg", true, Some("#43E57D")),
    ("mods/native-browser/sites/stackblitz.svg", true, Some("#3A8BEE")),
    ("mods/native-browser/sites/stackoverflow.svg", false, None),
    ("mods/native-browser/sites/statuspage.svg", true, Some("#5A85CE")),
    ("mods/native-browser/sites/storybook.svg", false, None),
    ("mods/native-browser/sites/stripe.svg", true, Some("#635BFF")),
    ("mods/native-browser/sites/substack.svg", true, Some("#FF6719")),
    ("mods/native-browser/sites/supabase.svg", false, None),
    ("mods/native-browser/sites/swagger.svg", true, Some("#85EA2D")),
    ("mods/native-browser/sites/tailscale.svg", true, None),
    ("mods/native-browser/sites/telegram.svg", false, None),
    ("mods/native-browser/sites/terraform.svg", false, None),
    ("mods/native-browser/sites/threads.svg", true, None),
    ("mods/native-browser/sites/tldraw.svg", true, None),
    ("mods/native-browser/sites/trello.svg", false, None),
    ("mods/native-browser/sites/turso.svg", true, Some("#4FF8D2")),
    ("mods/native-browser/sites/twilio.svg", true, Some("#F22F46")),
    ("mods/native-browser/sites/twitch.svg", true, Some("#9146FF")),
    ("mods/native-browser/sites/unsplash.svg", true, None),
    ("mods/native-browser/sites/upstash.svg", false, None),
    ("mods/native-browser/sites/v0.svg", true, None),
    ("mods/native-browser/sites/vercel.svg", true, None),
    ("mods/native-browser/sites/webflow.svg", true, Some("#3280F6")),
    ("mods/native-browser/sites/whatsapp.svg", false, None),
    ("mods/native-browser/sites/wikipedia.svg", true, None),
    ("mods/native-browser/sites/x.svg", true, None),
    ("mods/native-browser/sites/ycombinator.svg", true, Some("#F0652F")),
    ("mods/native-browser/sites/youtube.svg", false, None),
    ("mods/native-browser/sites/zendesk.svg", true, Some("#78A300")),
    ("mods/native-browser/sites/zoom.svg", false, None),
];

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

fn style_for(path: &'static str) -> SiteIcon {
    match ICON_STYLES.binary_search_by_key(&path, |&(p, _, _)| p) {
        Ok(i) => {
            let (p, tint, color) = ICON_STYLES[i];
            SiteIcon { path: p, tint, color }
        }
        Err(_) => SiteIcon { path, tint: true, color: None },
    }
}

fn host_matches(host: &str, domain: &str) -> bool {
    host == domain
        || (host.len() > domain.len()
            && host.ends_with(domain)
            && host.as_bytes()[host.len() - domain.len() - 1] == b'.')
}

pub fn site_icon(url: &str) -> SiteIcon {
    let Ok(parsed) = url::Url::parse(url.trim()) else {
        return DEFAULT;
    };
    if !matches!(parsed.scheme(), "http" | "https") {
        return DEFAULT;
    }
    let Some(raw_host) = parsed.host_str() else {
        return DEFAULT;
    };
    let lowered = raw_host.trim_end_matches('.').to_ascii_lowercase();
    let host = lowered.strip_prefix("www.").unwrap_or(&lowered);
    let path = parsed.path();

    if host_matches(host, "atlassian.net") {
        if path == "/wiki" || path.starts_with("/wiki/") {
            return style_for("mods/native-browser/sites/confluence.svg");
        }
        if path == "/jira"
            || path.starts_with("/jira/")
            || path.starts_with("/browse/")
            || path.starts_with("/projects/")
            || path.starts_with("/servicedesk")
            || path.starts_with("/secure/")
        {
            return style_for("mods/native-browser/sites/jira.svg");
        }
        return style_for("mods/native-browser/sites/atlassian.svg");
    }

    if host == "docs.google.com" {
        if path.starts_with("/spreadsheets") {
            return style_for("mods/native-browser/sites/googlesheets.svg");
        }
        if path.starts_with("/presentation") {
            return style_for("mods/native-browser/sites/googleslides.svg");
        }
        if path.starts_with("/forms") {
            return style_for("mods/native-browser/sites/googleforms.svg");
        }
        return style_for("mods/native-browser/sites/googledocs.svg");
    }

    for &(domain, icon) in SITE_RULES {
        if host_matches(host, domain) {
            return style_for(icon);
        }
    }

    for &(prefix, icon) in SELF_HOSTED_PREFIXES {
        if host.starts_with(prefix) {
            return style_for(icon);
        }
    }

    DEFAULT
}

pub fn pane_icon(url: &str, is_markdown: bool) -> SiteIcon {
    if is_markdown
        && let Ok(parsed) = url::Url::parse(url.trim())
        && matches!(parsed.host_str(), Some("127.0.0.1" | "localhost" | "::1"))
    {
        return style_for(MARKDOWN_ICON);
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
            ("https://linear.app/vibes/issue/ORC-123", "mods/native-browser/sites/linear.svg", true, Some("#5E6AD2")),
            ("https://www.linear.app/", "mods/native-browser/sites/linear.svg", true, Some("#5E6AD2")),
            ("https://gitlab.com/vibes/repos/studio/-/merge_requests/1", "mods/native-browser/sites/gitlab.svg", false, None),
            ("https://gitlab.internal.company.com/team/repo", "mods/native-browser/sites/gitlab.svg", false, None),
            ("https://github.com/Volpestyle/herdr-browser", "mods/native-browser/sites/github.svg", true, None),
            ("https://gist.github.com/user/123", "mods/native-browser/sites/github.svg", true, None),
            ("https://app.datadoghq.com/logs", "mods/native-browser/sites/datadog.svg", true, Some("#9B6BFF")),
            ("https://app.slack.com/client/T123/C456", "mods/native-browser/sites/slack.svg", false, None),
            ("https://acme.atlassian.net/browse/ENG-42", "mods/native-browser/sites/jira.svg", false, None),
            ("https://acme.atlassian.net/jira/software/projects/ENG", "mods/native-browser/sites/jira.svg", false, None),
            ("https://acme.atlassian.net/wiki/spaces/ENG/pages/1", "mods/native-browser/sites/confluence.svg", false, None),
            ("https://acme.atlassian.net/admin", "mods/native-browser/sites/atlassian.svg", false, None),
            ("https://docs.google.com/document/d/abc/edit", "mods/native-browser/sites/googledocs.svg", true, Some("#4285F4")),
            ("https://docs.google.com/spreadsheets/d/abc/edit", "mods/native-browser/sites/googlesheets.svg", true, Some("#34A853")),
            ("https://docs.google.com/presentation/d/abc/edit", "mods/native-browser/sites/googleslides.svg", true, Some("#FBBC04")),
            ("https://docs.google.com/forms/d/abc/viewform", "mods/native-browser/sites/googleforms.svg", true, Some("#9B6BFF")),
            ("https://mail.google.com/mail/u/0/#inbox", "mods/native-browser/sites/gmail.svg", false, None),
            ("https://calendar.google.com/calendar/u/0/r", "mods/native-browser/sites/googlecalendar.svg", false, None),
            ("https://meet.google.com/abc-defg-hij", "mods/native-browser/sites/googlemeet.svg", false, None),
            ("https://drive.google.com/drive/my-drive", "mods/native-browser/sites/googledrive.svg", false, None),
            ("https://www.google.com/search?q=rust", "mods/native-browser/sites/google.svg", false, None),
            ("https://us-east-1.console.aws.amazon.com/lambda/home", "mods/native-browser/sites/amazonaws.svg", true, Some("#FF9900")),
            ("https://d-123.awsapps.com/start", "mods/native-browser/sites/amazonaws.svg", true, Some("#FF9900")),
            ("https://www.amazon.com/dp/B000", "mods/native-browser/sites/amazon.svg", true, Some("#FFA929")),
            ("https://teams.microsoft.com/v2/", "mods/native-browser/sites/microsoftteams.svg", false, None),
            ("https://outlook.office.com/mail/", "mods/native-browser/sites/microsoftoutlook.svg", true, Some("#28A8EA")),
            ("https://acme.sharepoint.com/sites/eng", "mods/native-browser/sites/microsoftsharepoint.svg", true, Some("#038387")),
            ("https://portal.azure.com/#home", "mods/native-browser/sites/microsoftazure.svg", false, None),
            ("https://www.figma.com/design/abc/Spec", "mods/native-browser/sites/figma.svg", false, None),
            ("https://www.notion.so/Workspace-123", "mods/native-browser/sites/notion.svg", true, None),
            ("https://claude.ai/chat/123", "mods/native-browser/sites/claude.svg", true, Some("#D97757")),
            ("https://console.anthropic.com/settings", "mods/native-browser/sites/anthropic.svg", true, None),
            ("https://chatgpt.com/c/123", "mods/native-browser/sites/openai.svg", true, None),
            ("https://acme.okta.com/app/UserHome", "mods/native-browser/sites/okta.svg", true, Some("#2AA3E8")),
            ("https://grafana.prod.internal/d/abc", "mods/native-browser/sites/grafana.svg", false, None),
            ("https://kibana.corp.internal/app/discover", "mods/native-browser/sites/kibana.svg", false, None),
            ("https://argocd.corp.internal/applications", "mods/native-browser/sites/argo.svg", false, None),
        ];
        for (url, path, tint, color) in cases {
            assert_eq!(site_icon(url), SiteIcon { path, tint, color }, "url: {url}");
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
            assert_eq!(site_icon(url), DEFAULT, "url: {url}");
        }
    }

    #[test]
    fn markdown_preview_uses_markdown_icon_until_navigated_away() {
        assert_eq!(
            pane_icon("http://127.0.0.1:54321/token/README.md", true).path,
            MARKDOWN_ICON
        );
        assert_eq!(
            pane_icon("https://linear.app/vibes", true),
            SiteIcon {
                path: "mods/native-browser/sites/linear.svg",
                tint: true,
                color: Some("#5E6AD2"),
            }
        );
        assert_eq!(
            pane_icon("http://127.0.0.1:54321/token/README.md", false),
            DEFAULT
        );
    }

    #[test]
    fn every_referenced_site_icon_is_shipped_styled_and_valid() {
        assert!(ICON_STYLES.windows(2).all(|w| w[0].0 < w[1].0), "ICON_STYLES must be sorted");
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
        let styled: BTreeSet<&'static str> = ICON_STYLES.iter().map(|&(p, _, _)| p).collect();
        for icon in &referenced {
            assert!(shipped.contains(*icon), "icon referenced in rules is not shipped: {icon}");
            assert!(styled.contains(*icon), "icon referenced in rules has no style entry: {icon}");
        }
        for icon in &shipped {
            assert!(referenced.contains(icon.as_str()), "shipped site icon is never referenced: {icon}");
            assert!(styled.contains(icon.as_str()), "shipped site icon has no style entry: {icon}");
        }
        for &(path, tint, color) in ICON_STYLES {
            assert!(shipped.contains(path), "styled icon not shipped: {path}");
            if let Some(hex) = color {
                assert!(tint, "multicolor icon {path} should not also set tint color");
                assert!(crate::settings::parse_hex(hex).is_some(), "invalid hex {hex} for {path}");
            }
        }
    }
}

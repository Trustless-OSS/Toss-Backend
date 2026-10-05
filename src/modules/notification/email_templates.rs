/// Premium MJML-inspired email templates for Trustless OSS notifications
///
/// Layout follows MJML compilation output patterns:
/// - Full table-based structure (email-client safe, no flexbox/grid)
/// - 100% inline styles (Gmail strips <style> blocks)
/// - MSO conditional comments for Outlook rendering
/// - Trustless OSS brand: navy #0f172a, electric blue #2563eb, white #ffffff
///
/// HOW TO ADD/EDIT TEMPLATES:
/// 1. Add a function matching the notification kind name (e.g., `bounty_assigned()`)
/// 2. Pass in all necessary parameters from the notification data
/// 3. Return an EmailTemplate struct with subject and html content
/// 4. Update the `email_template_for()` function to map the kind to your function
/// 5. That's it! No other code changes needed.
use crate::modules::notification::kinds::Kind;
use serde_json::Value;

pub struct EmailTemplate {
    pub subject: String,
    pub html: String,
}

impl EmailTemplate {
    fn new(subject: &str, html: String) -> Self {
        Self {
            subject: subject.to_string(),
            html,
        }
    }
}

/// Returns the email template for a given notification kind
pub fn email_template_for(
    kind: Kind,
    title: &str,
    body: &str,
    data: &Value,
    app_url: &str,
) -> EmailTemplate {
    match kind {
        Kind::BountyAssigned => bounty_assigned(title, body, app_url),
        Kind::BountyUnassigned => bounty_unassigned(body, app_url),
        Kind::AmountUpdated => amount_updated(title, body, app_url),
        Kind::AmountMissing => amount_missing(title, body, app_url),
        Kind::WalletRequired => wallet_required(body, app_url),
        Kind::BountyLocked => bounty_locked(title, body, data, app_url),
        Kind::PayoutReleased => payout_released(title, body, data, app_url),
        Kind::PayoutBlocked => payout_blocked(title, body, app_url),
        Kind::BountyCancelled => bounty_cancelled(title, body, app_url),
        Kind::BountyRejected => bounty_rejected(title, body, app_url),
        Kind::InsufficientFunds => insufficient_funds(title, body, app_url),
        Kind::EscrowFunded => escrow_funded(title, body, app_url),
        Kind::EscrowRefunded => escrow_refunded(title, body, app_url),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// BRAND CONSTANTS
// ─────────────────────────────────────────────────────────────────────────────

const BRAND_NAVY: &str = "#0f172a";
const BRAND_BLUE: &str = "#2563eb";
const BRAND_BLUE_LIGHT: &str = "#3b82f6";
const BRAND_BG: &str = "#f1f5f9";
const BRAND_CARD: &str = "#ffffff";
const BRAND_BORDER: &str = "#e2e8f0";
const BRAND_TEXT: &str = "#334155";
const BRAND_MUTED: &str = "#64748b";
const BRAND_BANNER_URL: &str =
    "https://github.com/user-attachments/assets/511e24cb-87ed-40b8-9ca9-6a5f75f7139e";
const BRAND_LOGO_URL: &str =
    "https://github.com/user-attachments/assets/eb15200e-c4c5-4405-aca1-bbb692bd3480";

// ─────────────────────────────────────────────────────────────────────────────
// CORE LAYOUT ENGINE
// Produces fully MJML-compiled-style table email that works in all clients.
// ─────────────────────────────────────────────────────────────────────────────

struct LayoutConfig<'a> {
    /// Emoji or badge text shown in the accent pill next to h1
    badge: &'a str,
    /// Main heading inside the card
    title: &'a str,
    /// Inbox-preview snippet (hidden in body)
    preview: &'a str,
    /// Info box monospace content (the raw `body` string)
    info_box: &'a str,
    /// Descriptive paragraphs rendered above the info box
    lead: &'a str,
    /// Descriptive paragraphs rendered below the info box
    closing: Option<&'a str>,
    /// CTA button label + href
    cta_label: &'a str,
    cta_href: String,
    /// Accent color for the badge pill and the info-box left border
    accent: &'a str,
}

fn render_layout(cfg: LayoutConfig<'_>, app_url: &str) -> String {
    let closing_row = cfg.closing.map(|text| format!(
        r#"<tr>
          <td style="padding:0 0 20px 0;font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,Helvetica,Arial,sans-serif;font-size:15px;line-height:1.7;color:{text};">
            {text_content}
          </td>
        </tr>"#,
        text        = BRAND_TEXT,
        text_content = text,
    )).unwrap_or_default();

    format!(
        r#"<!DOCTYPE html>
<html lang="en" xmlns="http://www.w3.org/1999/xhtml" xmlns:v="urn:schemas-microsoft-com:vml" xmlns:o="urn:schemas-microsoft-com:office:office">
<head>
  <meta charset="UTF-8" />
  <meta name="viewport" content="width=device-width,initial-scale=1" />
  <meta http-equiv="X-UA-Compatible" content="IE=edge" />
  <meta name="x-apple-disable-message-reformatting" />
  <meta name="format-detection" content="telephone=no,date=no,address=no,email=no,url=no" />
  <title>{title}</title>
  <!--[if mso]>
  <noscript><xml><o:OfficeDocumentSettings><o:PixelsPerInch>96</o:PixelsPerInch></o:OfficeDocumentSettings></xml></noscript>
  <![endif]-->
  <style>
    /* Minimal head-style: only things that MUST stay in <style> */
    @media only screen and (max-width:600px) {{
      .outer-table  {{ width:100% !important; }}
      .card-table   {{ width:100% !important; border-radius:0 !important; }}
      .pad          {{ padding:28px 20px !important; }}
      .banner-img   {{ height:auto !important; width:100% !important; }}
      .btn-cell     {{ padding:24px 0 4px !important; }}
      h1            {{ font-size:22px !important; }}
    }}
  </style>
</head>
<body style="margin:0;padding:0;background-color:{bg};-webkit-text-size-adjust:100%;-ms-text-size-adjust:100%;">

<!-- Inbox preview text (hidden) -->
<div style="display:none;max-height:0;overflow:hidden;mso-hide:all;font-size:1px;line-height:1px;color:{bg};">{preview}&nbsp;&#847;&zwnj;&nbsp;&#847;&zwnj;&nbsp;&#847;&zwnj;&nbsp;&#847;&zwnj;&nbsp;&#847;&zwnj;</div>

<!-- Outer wrapper table -->
<table role="presentation" cellpadding="0" cellspacing="0" border="0" width="100%" style="background-color:{bg};border-collapse:collapse;">
  <tr>
    <td align="center" style="padding:40px 16px;">

      <!--[if mso]>
      <table role="presentation" width="600" border="0" cellpadding="0" cellspacing="0"><tr><td>
      <![endif]-->

      <!-- Card -->
      <table class="card-table outer-table" role="presentation" cellpadding="0" cellspacing="0" border="0" width="600"
             style="background-color:{card};border-radius:16px;border:1px solid {border};border-collapse:collapse;overflow:hidden;box-shadow:0 4px 24px rgba(15,23,42,0.08);">

        <!-- ── BANNER ── -->
        <tr>
          <td style="padding:0;line-height:0;font-size:0;">
            <a href="{app_url}" style="display:block;line-height:0;">
              <img class="banner-img"
                   src="{banner_url}"
                   alt="Trustless OSS"
                   width="600"
                   style="display:block;width:600px;max-width:100%;height:auto;border:0;outline:none;" />
            </a>
          </td>
        </tr>

        <!-- ── GRADIENT DIVIDER ── -->
        <tr>
          <td style="padding:0;line-height:4px;font-size:4px;background:linear-gradient(90deg,{navy} 0%,{blue} 50%,{blue_light} 100%);height:4px;">&#8203;</td>
        </tr>

        <!-- ── BODY PAD ── -->
        <tr>
          <td class="pad" style="padding:40px 48px 12px;">

            <!-- Logo lockup -->
            <table role="presentation" cellpadding="0" cellspacing="0" border="0">
              <tr>
                <td style="vertical-align:middle;padding-right:12px;">
                  <img src="{logo_url}"
                       alt="TOSS Logo"
                       width="40" height="40"
                       style="display:block;width:40px;height:40px;border-radius:8px;border:0;" />
                </td>
                <td style="vertical-align:middle;">
                  <span style="font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,Helvetica,Arial,sans-serif;font-size:16px;font-weight:700;color:{navy};letter-spacing:-0.3px;">
                    Trustless <span style="color:{blue};">OSS</span>
                  </span>
                </td>
              </tr>
            </table>

            <!-- Badge + heading -->
            <table role="presentation" cellpadding="0" cellspacing="0" border="0" style="margin-top:32px;">
              <tr>
                <td>
                  <!-- Badge pill -->
                  <div style="display:inline-block;background-color:{accent}1a;border:1px solid {accent}33;border-radius:100px;padding:4px 14px;margin-bottom:16px;">
                    <span style="font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,Helvetica,Arial,sans-serif;font-size:13px;font-weight:600;color:{accent};letter-spacing:0.3px;">{badge}</span>
                  </div>
                  <!-- H1 -->
                  <h1 style="margin:0;padding:0;font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,Helvetica,Arial,sans-serif;font-size:26px;font-weight:700;color:{navy};line-height:1.25;letter-spacing:-0.5px;">{title}</h1>
                </td>
              </tr>
            </table>

          </td>
        </tr>

        <!-- ── CONTENT ── -->
        <tr>
          <td class="pad" style="padding:24px 48px 0;">
            <table role="presentation" cellpadding="0" cellspacing="0" border="0" width="100%">

              <!-- Lead paragraph -->
              <tr>
                <td style="padding:0 0 20px 0;font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,Helvetica,Arial,sans-serif;font-size:15px;line-height:1.7;color:{text};">
                  {lead}
                </td>
              </tr>

              <!-- Info box (monospace highlight) -->
              <tr>
                <td style="padding:0 0 20px 0;">
                  <table role="presentation" cellpadding="0" cellspacing="0" border="0" width="100%"
                         style="background-color:#f8fafc;border:1px solid {border};border-left:4px solid {accent};border-radius:8px;">
                    <tr>
                      <td style="padding:16px 20px;font-family:ui-monospace,SFMono-Regular,Menlo,Monaco,Consolas,'Liberation Mono','Courier New',monospace;font-size:13px;line-height:1.6;color:{navy};white-space:pre-wrap;word-break:break-word;">
                        {info_box}
                      </td>
                    </tr>
                  </table>
                </td>
              </tr>

              <!-- Closing paragraph (optional) -->
              {closing_row}

              <!-- CTA Button -->
              <tr>
                <td class="btn-cell" style="padding:8px 0 40px;">
                  <!--[if mso]>
                  <v:roundrect xmlns:v="urn:schemas-microsoft-com:vml" xmlns:w="urn:schemas-microsoft-com:office:word"
                    href="{cta_href}" style="height:48px;v-text-anchor:middle;width:220px;" arcsize="12%"
                    stroke="f" fillcolor="{navy}">
                    <w:anchorlock/>
                    <center style="color:#ffffff;font-family:sans-serif;font-size:15px;font-weight:700;">{cta_label}</center>
                  </v:roundrect>
                  <![endif]-->
                  <!--[if !mso]><!-->
                  <a href="{cta_href}"
                     style="display:inline-block;background-color:{navy};color:#ffffff;font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,Helvetica,Arial,sans-serif;font-size:15px;font-weight:600;letter-spacing:0.2px;text-decoration:none;padding:13px 32px;border-radius:8px;border:2px solid {navy};">
                    {cta_label}
                  </a>
                  <!--<![endif]-->
                </td>
              </tr>

            </table>
          </td>
        </tr>

        <!-- ── FOOTER ── -->
        <tr>
          <td style="background-color:#f8fafc;border-top:1px solid {border};padding:28px 48px;border-radius:0 0 16px 16px;">
            <table role="presentation" cellpadding="0" cellspacing="0" border="0" width="100%">
              <tr>
                <td align="center" style="font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,Helvetica,Arial,sans-serif;font-size:13px;color:{muted};line-height:1.6;">
                  <p style="margin:0 0 12px 0;">
                    Need help? <a href="{app_url}/support" style="color:{blue};text-decoration:none;font-weight:600;">Contact Support</a>
                  </p>
                  <p style="margin:0 0 16px 0;">
                    <a href="{app_url}/dashboard" style="color:{muted};text-decoration:none;margin:0 10px;">Dashboard</a>
                    <span style="color:{border};">•</span>
                    <a href="{app_url}/settings/notifications" style="color:{muted};text-decoration:none;margin:0 10px;">Notification Settings</a>
                    <span style="color:{border};">•</span>
                    <a href="{app_url}/bounties" style="color:{muted};text-decoration:none;margin:0 10px;">Bounties</a>
                  </p>
                  <p style="margin:0;font-size:12px;color:#94a3b8;">
                    © 2025 Trustless OSS · Open Source, Trustless Rewards
                  </p>
                </td>
              </tr>
            </table>
          </td>
        </tr>

      </table>
      <!--[if mso]></td></tr></table><![endif]-->

    </td>
  </tr>
</table>
</body>
</html>"#,
        title = cfg.title,
        preview = cfg.preview,
        bg = BRAND_BG,
        card = BRAND_CARD,
        border = BRAND_BORDER,
        navy = BRAND_NAVY,
        blue = BRAND_BLUE,
        blue_light = BRAND_BLUE_LIGHT,
        text = BRAND_TEXT,
        muted = BRAND_MUTED,
        accent = cfg.accent,
        badge = cfg.badge,
        lead = cfg.lead,
        info_box = cfg.info_box,
        closing_row = closing_row,
        cta_label = cfg.cta_label,
        cta_href = cfg.cta_href,
        app_url = app_url,
        banner_url = BRAND_BANNER_URL,
        logo_url = BRAND_LOGO_URL,
    )
}

// ─────────────────────────────────────────────────────────────────────────────
// BOUNTY NOTIFICATIONS
// ─────────────────────────────────────────────────────────────────────────────

fn bounty_assigned(title: &str, body: &str, app_url: &str) -> EmailTemplate {
    let html = render_layout(LayoutConfig {
        badge:     "🎯 New Assignment",
        title,
        preview:   "You've been assigned to a bounty — start contributing!",
        info_box:  body,
        lead:      "You've been successfully assigned to work on this bounty. Start whenever you're ready.",
        closing:   Some("Your payout will be released automatically once your pull request is merged and approved by the maintainers."),
        cta_label: "View Bounty →",
        cta_href:  format!("{}/bounties", app_url),
        accent:    BRAND_BLUE,
    }, app_url);
    EmailTemplate::new(title, html)
}

fn bounty_unassigned(body: &str, app_url: &str) -> EmailTemplate {
    let title = "Bounty Unassigned";
    let html = render_layout(LayoutConfig {
        badge:     "📋 Bounty Update",
        title,
        preview:   "A bounty has been unassigned and is now open for contributors.",
        info_box:  body,
        lead:      "A previously assigned bounty has been unassigned and is now open for other contributors to claim.",
        closing:   Some("Browse other open bounties and jump into something new!"),
        cta_label: "Browse Open Bounties →",
        cta_href:  format!("{}/bounties", app_url),
        accent:    "#8b5cf6",
    }, app_url);
    EmailTemplate::new(title, html)
}

fn bounty_locked(title: &str, body: &str, _data: &Value, app_url: &str) -> EmailTemplate {
    let html = render_layout(LayoutConfig {
        badge:     "🔒 Escrow Secured",
        title,
        preview:   "Your bounty funds are secured in escrow and ready.",
        info_box:  body,
        lead:      "Your bounty funds have been secured in escrow on the Stellar network.",
        closing:   Some("Once you merge the linked pull request, your payout will be released automatically."),
        cta_label: "Track Progress →",
        cta_href:  format!("{}/bounties", app_url),
        accent:    "#0891b2",
    }, app_url);
    EmailTemplate::new(title, html)
}

fn bounty_cancelled(title: &str, body: &str, app_url: &str) -> EmailTemplate {
    let html = render_layout(
        LayoutConfig {
            badge: "❌ Bounty Cancelled",
            title,
            preview: "A bounty you were tracking has been cancelled.",
            info_box: body,
            lead: "A bounty you were tracking has been cancelled by the maintainers.",
            closing: Some(
                "Don't worry — there are many other open issues waiting for great contributions.",
            ),
            cta_label: "Find More Bounties →",
            cta_href: format!("{}/bounties", app_url),
            accent: "#dc2626",
        },
        app_url,
    );
    EmailTemplate::new(title, html)
}

fn bounty_rejected(title: &str, body: &str, app_url: &str) -> EmailTemplate {
    let html = render_layout(LayoutConfig {
        badge:     "⚠️ Review Required",
        title,
        preview:   "Your submitted work needs some changes — check the feedback.",
        info_box:  body,
        lead:      "Your submitted work requires some changes or has been rejected by the maintainers.",
        closing:   Some("Check the issue for detailed feedback and feel free to submit revisions at any time."),
        cta_label: "View Feedback →",
        cta_href:  format!("{}/bounties", app_url),
        accent:    "#d97706",
    }, app_url);
    EmailTemplate::new(title, html)
}

// ─────────────────────────────────────────────────────────────────────────────
// AMOUNT NOTIFICATIONS
// ─────────────────────────────────────────────────────────────────────────────

fn amount_updated(title: &str, body: &str, app_url: &str) -> EmailTemplate {
    let html = render_layout(LayoutConfig {
        badge:     "💰 Reward Updated",
        title,
        preview:   "The reward amount for a bounty you're watching has changed.",
        info_box:  body,
        lead:      "The reward amount for a bounty you are watching has been updated by the maintainers.",
        closing:   None,
        cta_label: "View Update →",
        cta_href:  format!("{}/bounties", app_url),
        accent:    "#059669",
    }, app_url);
    EmailTemplate::new(title, html)
}

fn amount_missing(title: &str, body: &str, app_url: &str) -> EmailTemplate {
    let html = render_layout(LayoutConfig {
        badge:     "⏳ Pending Reward",
        title,
        preview:   "A bounty has a pending reward — maintainers are working on it.",
        info_box:  body,
        lead:      "A bounty currently has a pending reward amount. The maintainers are working on setting the final reward.",
        closing:   None,
        cta_label: "Check Status →",
        cta_href:  format!("{}/bounties", app_url),
        accent:    "#d97706",
    }, app_url);
    EmailTemplate::new(title, html)
}

fn insufficient_funds(title: &str, body: &str, app_url: &str) -> EmailTemplate {
    let html = render_layout(
        LayoutConfig {
            badge: "⚠️ Action Required",
            title,
            preview: "Your escrow pool needs a top-up to continue.",
            info_box: body,
            lead: "There is an issue with the escrow balance on your repository.",
            closing: Some(
                "The escrow pool needs to be topped up before bounty payouts can proceed.",
            ),
            cta_label: "View Dashboard →",
            cta_href: format!("{}/dashboard", app_url),
            accent: "#dc2626",
        },
        app_url,
    );
    EmailTemplate::new(title, html)
}

// ─────────────────────────────────────────────────────────────────────────────
// WALLET & PAYOUT NOTIFICATIONS
// ─────────────────────────────────────────────────────────────────────────────

fn wallet_required(body: &str, app_url: &str) -> EmailTemplate {
    let title = "Wallet Required for Payout";
    let html = render_layout(LayoutConfig {
        badge:     "🔑 Action Required",
        title,
        preview:   "Connect your Stellar wallet to receive your payout.",
        info_box:  body,
        lead:      "Your payout is ready! To receive your funds, you need to connect a Stellar wallet to your account.",
        closing:   Some("Once connected, your payout will be released automatically to your secure destination."),
        cta_label: "Connect Wallet Now →",
        cta_href:  format!("{}/settings/wallet", app_url),
        accent:    BRAND_BLUE,
    }, app_url);
    EmailTemplate::new(title, html)
}

fn payout_released(title: &str, body: &str, _data: &Value, app_url: &str) -> EmailTemplate {
    let html = render_layout(LayoutConfig {
        badge:     "✨ Payout Sent",
        title,
        preview:   "Congratulations! Your bounty payout has been released.",
        info_box:  body,
        lead:      "Congratulations! Your payout has been successfully released to your Stellar wallet.",
        closing:   Some("Thank you for your amazing contribution to open source. You rock! 🚀"),
        cta_label: "View Payout Details →",
        cta_href:  format!("{}/dashboard", app_url),
        accent:    "#059669",
    }, app_url);
    EmailTemplate::new(title, html)
}

fn payout_blocked(title: &str, body: &str, app_url: &str) -> EmailTemplate {
    let html = render_layout(
        LayoutConfig {
            badge: "🛑 Payout Blocked",
            title,
            preview: "There was an issue releasing your payout — we're on it.",
            info_box: body,
            lead: "There was an issue releasing your payout. Please review the details below.",
            closing: Some("Our support team is ready to help resolve this as quickly as possible."),
            cta_label: "Contact Support →",
            cta_href: format!("{}/support", app_url),
            accent: "#dc2626",
        },
        app_url,
    );
    EmailTemplate::new(title, html)
}

// ─────────────────────────────────────────────────────────────────────────────
// ESCROW NOTIFICATIONS
// ─────────────────────────────────────────────────────────────────────────────

fn escrow_funded(title: &str, body: &str, app_url: &str) -> EmailTemplate {
    let html = render_layout(
        LayoutConfig {
            badge: "💸 Escrow Funded",
            title,
            preview: "Your repository escrow has been funded — create bounties now!",
            info_box: body,
            lead: "Your repository's escrow has been successfully funded on the Stellar network.",
            closing: Some(
                "You can now create bounties and start assigning work to contributors securely.",
            ),
            cta_label: "Create Bounty →",
            cta_href: format!("{}/bounties/create", app_url),
            accent: "#059669",
        },
        app_url,
    );
    EmailTemplate::new(title, html)
}

fn escrow_refunded(title: &str, body: &str, app_url: &str) -> EmailTemplate {
    let html = render_layout(LayoutConfig {
        badge:     "↩️ Escrow Refunded",
        title,
        preview:   "Your escrow pool has been refunded successfully.",
        info_box:  body,
        lead:      "Your escrow pool has been refunded successfully.",
        closing:   Some("Any open bounties have been cancelled and the remaining funds have been returned to your account."),
        cta_label: "View Account →",
        cta_href:  format!("{}/dashboard", app_url),
        accent:    "#8b5cf6",
    }, app_url);
    EmailTemplate::new(title, html)
}

// ─────────────────────────────────────────────────────────────────────────────
// TESTS
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_valid_html() {
        let template =
            bounty_assigned("Test Bounty", "You got assigned", "https://app.example.com");
        assert!(template.html.contains("<!DOCTYPE html>"));
        assert!(template.html.contains("</html>"));
        assert!(template.html.contains("https://app.example.com/bounties"));
        assert!(template.html.contains("Trustless"));
        assert!(template.html.contains("role=\"presentation\""));
    }

    #[test]
    fn all_kinds_have_templates() {
        let data = serde_json::json!({});
        let app_url = "https://app.example.com";

        let _t1 = email_template_for(Kind::BountyAssigned, "Title", "Body", &data, app_url);
        let _t2 = email_template_for(Kind::BountyUnassigned, "Title", "Body", &data, app_url);
        let _t3 = email_template_for(Kind::AmountUpdated, "Title", "Body", &data, app_url);
        let _t4 = email_template_for(Kind::AmountMissing, "Title", "Body", &data, app_url);
        let _t5 = email_template_for(Kind::WalletRequired, "Title", "Body", &data, app_url);
        let _t6 = email_template_for(Kind::BountyLocked, "Title", "Body", &data, app_url);
        let _t7 = email_template_for(Kind::PayoutReleased, "Title", "Body", &data, app_url);
        let _t8 = email_template_for(Kind::PayoutBlocked, "Title", "Body", &data, app_url);
        let _t9 = email_template_for(Kind::BountyCancelled, "Title", "Body", &data, app_url);
        let _t10 = email_template_for(Kind::BountyRejected, "Title", "Body", &data, app_url);
        let _t11 = email_template_for(Kind::InsufficientFunds, "Title", "Body", &data, app_url);
        let _t12 = email_template_for(Kind::EscrowFunded, "Title", "Body", &data, app_url);
        let _t13 = email_template_for(Kind::EscrowRefunded, "Title", "Body", &data, app_url);
    }

    #[test]
    fn has_mso_conditionals() {
        let data = serde_json::json!({});
        let template =
            payout_released("Payout!", "100 USDC sent", &data, "https://app.example.com");
        // Ensures Outlook VML button is present
        assert!(template.html.contains("v:roundrect"));
        assert!(template.html.contains("mso-hide:all"));
    }

    #[test]
    fn has_brand_assets() {
        let template = escrow_funded("Funded", "5000 USDC", "https://app.example.com");
        assert!(template.html.contains(BRAND_BANNER_URL));
        assert!(template.html.contains(BRAND_LOGO_URL));
        assert!(template.html.contains(BRAND_NAVY));
        assert!(template.html.contains(BRAND_BLUE));
    }
}

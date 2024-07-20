use reqwest::header::USER_AGENT;
use scraper::{Html, Selector};
use std::env;
use std::fmt::Display;
use std::fs::File;
use std::io::prelude::*;

use lettre::message::{header, Mailbox, MultiPart, SinglePart};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Message, SmtpTransport, Transport};

use reqwest::{self, Url};

const TARGET_URL_KEY: &str = "TARGET_URL";
const SEARCH_TEXT_KEY: &str = "SEARCH_TEXT";
const CONTENT_TYPE_KEY: &str = "CONTENT_TYPE";
const SELECTOR_KEY: &str = "SELECTOR";
const DEBUG_KEY: &str = "DEBUG";
const PREVENT_EMAIL_KEY: &str = "PREVENT_EMAIL";
const SMTP_USER_KEY: &str = "SMTP_USER";
const SMTP_PASS_KEY: &str = "SMTP_PASS";
const SMTP_RELAY_KEY: &str = "SMTP_RELAY";
const EMAIL_TO_KEY: &str = "EMAIL_TO";
const EMAIL_FROM_KEY: &str = "EMAIL_FROM";

// Expansions:
// regex
// xpath
// jq structure
// json type
// csv ..

#[tokio::main]
async fn main() {
    dotenv::dotenv().ok();
    let is_debug = env::var(DEBUG_KEY).is_ok();
    let prevent_email = env::var(PREVENT_EMAIL_KEY).is_ok();

    let config = load_config();
    let content = download_content(&config, is_debug).await;

    if is_debug {
        println!("Content {}", content);
        let mut f = File::create("content.html").unwrap();
        f.write_all(content.as_bytes()).unwrap();
        f.sync_data().unwrap();
    }

    let matches = match config.content_type {
        ContentType::Html => parse_html_and_search(&content, &config),
        ContentType::Text => search_for_text(&content, &config),
    };

    if is_debug {
        println!("Found {} matche(s)", matches.len());
        println!("\nResults");
        for result in &matches {
            println!("\n{}\n", result);
        }
    }

    if !matches.is_empty() {
        email_result(&matches, &config, is_debug, prevent_email);
    } else {
        println!("No matches");
    }

    println!("Finished");
}

fn email_result(matches: &[String], config: &Config, is_debug: bool, prevent_email: bool) {
    let url = &config.url;
    let subject = format!("Found {} matche(s) for {}", matches.len(), url);

    let mut html_body = String::from(
        r#"<!DOCTYPE html>
        <html lang="en">
        <head>
        <meta charset="UTF-8">
        <meta name="viewport" content="width=device-width, initial-scale=1.0">
        <style>
            .container {
                max-width: 440px;
            }
            .container img {
                max-width: 440px;
                height: unset;
            }
        </style>
    "#,
    );

    html_body.push_str("<title>");
    html_body.push_str(&subject);
    html_body.push_str("</title>");
    html_body.push_str("</head>");
    html_body.push_str("<body>");
    html_body.push_str(&format!("<h2><a class=\"url\" href=\"{}\">", url,));
    html_body.push_str(&url.to_string());
    html_body.push_str("</h2></a><br>");
    html_body.push_str("<table class=\"container\"><tbody>");
    html_body.push_str(
        &matches
            .iter()
            .map(|match_body| format!("<td>{}<td>", match_body))
            .collect::<Vec<String>>()
            .join("<br/>"),
    );
    html_body.push_str("</tbody></table>");
    html_body.push_str("</body>");
    html_body.push_str("</html>");

    let email = Message::builder()
        .from(config.email_from.clone())
        .to(config.email_to.clone())
        .subject(subject)
        .multipart(
            MultiPart::alternative()
                .singlepart(
                    SinglePart::builder()
                        .header(header::ContentType::TEXT_PLAIN)
                        .body(format!("Results:\n{}", matches.join("\n---\n"))),
                )
                .singlepart(
                    SinglePart::builder()
                        .header(header::ContentType::TEXT_HTML)
                        .body(html_body.clone()),
                ),
        )
        .unwrap();

    if is_debug {
        println!("Email {}", html_body);
        let mut f = File::create("email.html").unwrap();
        f.write_all(html_body.as_bytes()).unwrap();
        f.sync_data().unwrap();
    }

    if !prevent_email {
        send_email(&email);
    }
}

fn email_error(error: &str, config: &Config) {
    let email = Message::builder()
        .from(config.email_from.clone())
        .to(config.email_to.clone())
        .subject(format!("Error polling site {}", config.url))
        .body(format!("Error: \n{}", error))
        .unwrap();

    send_email(&email);
}

fn send_email(email: &Message) {
    let smtp_relay = env::var(SMTP_RELAY_KEY).expect("Need SMTP relay url");
    let smtp_user = env::var(SMTP_USER_KEY).expect("Need SMTP username");
    let smtp_pass = env::var(SMTP_PASS_KEY).expect("Need SMTP password");

    let creds = Credentials::new(smtp_user, smtp_pass);

    // Open a remote connection to gmail
    let mailer = SmtpTransport::relay(&smtp_relay)
        .unwrap()
        .credentials(creds)
        .build();

    match mailer.send(&email) {
        Ok(_) => println!("Email sent"),
        Err(e) => panic!("Error sending email {}", e),
    };
}

fn search_for_text(content: &str, config: &Config) -> Vec<String> {
    let mut results = Vec::new();

    let lines = content
        .split('\n')
        .map(|part| part.to_string())
        .collect::<Vec<String>>();

    config
        .search_terms
        .as_ref()
        .unwrap()
        .iter()
        .for_each(|term| {
            for (index, line) in lines.iter().enumerate() {
                if line.contains(term) {
                    results.push(
                        [
                            lines[index - 1].clone(),
                            line.clone(),
                            lines[index + 1].clone(),
                        ]
                        .join("\n"),
                    );
                }
            }
        });

    results
}

fn parse_html_and_search(content: &str, config: &Config) -> Vec<String> {
    let document = Html::parse_document(content);
    let selector = config
        .selector
        .as_ref()
        .map(|selector| Selector::parse(selector).expect("Unable to parse selector"));

    let mut results = Vec::new();
    let origin = config.url.origin();

    if let Some(selector) = selector {
        for element in document.select(&selector) {
            // assume is the url is relatively defined we use the origin of target
            let mut element_html = element.html();
            element_html = element_html.replace(
                "href=\"/",
                &format!("href=\"{}/", origin.ascii_serialization()),
            );
            element_html = element_html.replace("src=\"//", &format!("src=\"{}//", "https:"));

            // todo handle scheme-less  //domain.com/image.png urls
            let srcset_offset = element_html.find("srcset");
            if let Some(offset) = srcset_offset {
                let beg = offset + "srcset=\"".len();
                let offset_end = element_html[beg..].find("\"").map(|i| beg + i).unwrap() + 1;
                element_html.replace_range(offset..offset_end, "");
            }

            results.push(element_html);
        }
    }
    results
}

async fn download_content(config: &Config, is_debug: bool) -> String {
    let client = reqwest::Client::new();

    // pretending to be google bot helps make sure we get a server-side rendered version of the app
    let data = client.get(config.url.to_string()).header(USER_AGENT, "Mozilla/5.0 AppleWebKit/537.36 (KHTML, like Gecko; compatible; Googlebot/2.1; +http://www.google.com/bot.html) Chrome/W.X.Y.Z Safari/537.36").send().await;

    if is_debug {
        println!("{:?}", data);
    }

    if data.is_err() {
        let error = data.unwrap_err();
        email_error(&format!("Error fetching: {}", &error.to_string()), config);
        panic!("Unable to fetch {}", error);
    }

    match data.unwrap().text().await {
        Ok(body) => body,
        Err(error) => {
            email_error(
                &format!("Error unwrapping body: {}", &error.to_string()),
                config,
            );
            panic!("Error parsing {}", error);
        }
    }
}

struct Config {
    content_type: ContentType,
    url: Url,
    search_terms: Option<Vec<String>>,
    selector: Option<String>,
    email_to: Mailbox,
    email_from: Mailbox,
}

fn load_config() -> Config {
    let url_string = env::var(TARGET_URL_KEY).expect("Please define TARGET_URL in .env");
    let url = Url::parse(&url_string).expect("Invalid URL");
    println!("Polling {} ", &url_string);

    let content_type_string =
        env::var(CONTENT_TYPE_KEY).expect("Please define CONTENT_TYPE in .env");
    let content_type = ContentType::try_from(&content_type_string)
        .unwrap_or_else(|_| panic!("Unknown content type {}", content_type_string));

    println!("for '{}' content", content_type);

    let (search_terms, selector) = match content_type {
        ContentType::Html => {
            let selector = env::var(SELECTOR_KEY)
                .expect("Please supply SELECTOR in .env for HTML content type");
            println!("using selector: {}", &selector);
            (None, Some(selector))
        }
        ContentType::Text => {
            let search_text = env::var(SEARCH_TEXT_KEY)
                .expect("Please define SEARCH_TEXT in .env as comma separated entries");
            println!("using search_text: {}", search_text);

            let search_terms = search_text
                .split(',')
                .map(|term| term.to_owned())
                .collect::<Vec<String>>();
            (Some(search_terms), None)
        }
    };

    let email_to_string =
        env::var(EMAIL_TO_KEY).expect("Need EMAIL_TO user email: 'User <user@example.com>'");
    let email_to = email_to_string
        .parse::<Mailbox>()
        .expect("Need user of form: 'User <user@example.com>'");

    let email_from_string =
        env::var(EMAIL_FROM_KEY).expect("Need EMAIL_FROM user email: 'App Name <app@example.com>'");
    let email_from = email_from_string
        .parse::<Mailbox>()
        .expect("Need user of form: 'App Name <app@example.com>'");

    // doing this upfront so we can exit early
    let _ = env::var(SMTP_RELAY_KEY).expect("Need SMTP relay url");
    let _ = env::var(SMTP_USER_KEY).expect("Need SMTP username");
    let _ = env::var(SMTP_PASS_KEY).expect("Need SMTP password");

    Config {
        url,
        search_terms,
        content_type,
        selector,
        email_from,
        email_to,
    }
}

enum ContentType {
    Html,
    Text,
}

impl TryFrom<&String> for ContentType {
    type Error = &'static str;

    fn try_from(value: &String) -> Result<Self, Self::Error> {
        match value.to_lowercase().trim() {
            "html" => Ok(ContentType::Html),
            "text" => Ok(ContentType::Text),
            _ => Err("Unknown content type"),
        }
    }
}

impl Display for ContentType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let content_type_string = match self {
            ContentType::Html => "HTML",
            ContentType::Text => "text",
        };
        f.write_fmt(format_args!("{content_type_string}"))
    }
}

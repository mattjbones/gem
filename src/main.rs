use std::env;
use std::fmt::Display;
use std::fs::{self, remove_file, File};
use std::io::prelude::*;
use std::path::Path;

use chrono::{prelude::*, Duration};
use scraper::{Html, Selector};

use lettre::message::{header, Mailbox, MultiPart, SinglePart};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Message, SmtpTransport, Transport};

use reqwest::header::USER_AGENT;
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
const EMAIL_MAX_PER_INTERVAL_KEY: &str = "EMAIL_MAX_PER_INTERVAL";
const EMAIL_INTERVAL_S_KEY: &str = "EMAIL_INTERVAL_S";

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
    if is_debug {
        fs::create_dir("tmp").expect("can't create debug dir");
    }
    let prevent_email = env::var(PREVENT_EMAIL_KEY).is_ok();

    let config = load_config();
    let content = download_content(&config, is_debug).await;

    if is_debug {
        println!("Content {}", content);
        let mut f = File::create("tmp/content.html").unwrap();
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

    let has_matches = !matches.is_empty();

    if !has_matches {
        println!("No matches");
    } else if check_last_send_time(&config, is_debug).unwrap_or(false) {
        email_result(&matches, &config, is_debug, prevent_email);
    }

    println!("Finished");
}

const DEFAULT_EMAIL_INTERVAL: u32 = 60 * 5; //5 minutes
const DEFAULT_MAX_SEND: u8 = 3;
fn check_last_send_time(config: &Config, is_debug: bool) -> std::io::Result<bool> {
    let email_interval = env::var(EMAIL_INTERVAL_S_KEY).map_or(DEFAULT_EMAIL_INTERVAL, |val| {
        val.parse::<u32>()
            .expect("Invalid number for email interval")
    });
    let max_sent = env::var(EMAIL_MAX_PER_INTERVAL_KEY).map_or(DEFAULT_MAX_SEND, |val| {
        val.parse::<u8>().expect("Invalid number for max email")
    });

    let filename = format!("last_checked-{}", config.url.domain().or(Some("")).unwrap());

    if is_debug {
        println!("{}", &filename);
    }

    if !Path::new(&filename).exists() {
        save_last_send_time(&filename, 1)?;
        return Ok(true);
    }

    // load file
    let mut file = File::open(&filename)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    let parts = contents.split("|").collect::<Vec<&str>>();

    if parts.len() != 2 {
        // can't parse file so log it, delete and send email
        println!("Unexpected format {}", contents);
        remove_file(&filename)?;
        return Ok(true);
    }

    let last_send_time_seconds = parts
        .get(0)
        .map(|val| {
            val.parse::<i64>()
                .expect(&format!("Unable to parse timestamp to int {}", val))
        })
        .unwrap();

    println!("{:?}", parts);

    let last_send_time =
        DateTime::from_timestamp(last_send_time_seconds, 0).expect("Unable to create DateTime");

    let total_send_count = parts
        .get(1)
        .map(|val| val.parse::<u8>().expect("Unable to parse count"))
        .unwrap();

    let is_last_send_outside_interval =
        Utc::now() > last_send_time + Duration::seconds(email_interval.into());
    let is_total_lower_than_threshold = total_send_count < max_sent;

    if is_total_lower_than_threshold {
        save_last_send_time(&filename, total_send_count + 1)?;
        println!("sent {} out of {} emails", total_send_count, max_sent);
        Ok(true)
    } else if is_last_send_outside_interval {
        save_last_send_time(&filename, 1)?;
        println!("last sent more than {}s ago", email_interval);
        Ok(true)
    } else {
        println!("not sending email");
        Ok(false)
    }
}

fn save_last_send_time(filename: &str, count: u8) -> std::io::Result<bool> {
    let mut file = File::create(filename)?;
    file.write_all(format!("{}|{}", Utc::now().timestamp(), count).as_bytes())?;
    Ok(true)
}

fn email_result(matches: &[String], config: &Config, is_debug: bool, prevent_email: bool) {
    let url = &config.url;
    let subject = format!("Found {} match(es) for {}", matches.len(), url);

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
        let mut f = File::create("tmp/email.html").unwrap();
        f.write_all(html_body.as_bytes()).unwrap();
        f.sync_data().unwrap();
    }

    if !prevent_email {
        send_email(&email);
    } else {
        println!("PREVENT_EMAIL set - not sending email\n\n{}", html_body);
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

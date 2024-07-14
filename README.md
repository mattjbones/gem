# Gem 

A simple app to look for things in places

## Quickstart 

1. Setup

Create a .env with the following fields: 

```ini
TARGET_URL=https://example.com  

# for HTML content type a SELECTOR must be defined
CONTENT_TYPE=html
SELECTOR=h1

# for plain text search_text
# CONTENT_TYPE=text
# SEARCH_TEXT=example,text

# smtp details for emailing results
SMTP_RELAY=smtp.example.com
SMTP_PASS=example-pass
SMTP_USER=user@example.com

# configure the email sender and receiver 
EMAIL_TO=User <user@example.com>
EMAIL_FROM=App <app@example.com>

# Optional debug flag for more logging
# DEBUG=true
```

2. Build: 

`docker build -t hub/gem:latest .`

3. Run: 

`docker run --env-file=.env hub/gem:latest`

4. (Optional) Generate env string

I deploy this as a cronjob using [Ofelia](https://github.com/mcuadros/ofelia/tree/master) which can take label arguments as a string array.

To generate this string run `node generate_env_string.mjs` and it'll output something that can be added to docker-compose yaml. 
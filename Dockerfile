FROM rust:1.70 as builder
WORKDIR /usr/src/gem

# used to help cache the build deps step 
COPY dummy.rs .
COPY Cargo.* .
RUN sed -i 's#src/main.rs#dummy.rs#' Cargo.toml
RUN cargo build --release
RUN sed -i 's#dummy.rs#src/main.rs#' Cargo.toml
COPY src src
RUN cargo install --path .

FROM debian:bullseye-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /usr/local/cargo/bin/gem /usr/local/bin/gem
CMD ["gem"]
 


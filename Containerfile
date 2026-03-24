FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        curl \
        build-essential \
        pkg-config \
        libssl-dev \
    && rm -rf /var/lib/apt/lists/*

ENV PATH="/root/.cargo/bin:${PATH}"

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal \
    && rustup toolchain install stable \
    && rustup default stable

WORKDIR /app

COPY . /app

RUN cargo build --release \
    && ln -s /app/target/release/yoclaw /usr/local/bin/yoclaw

CMD ["yoclaw"]

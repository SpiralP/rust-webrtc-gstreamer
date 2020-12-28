FROM debian:testing

RUN apt-get -y update \
    && export DEBIAN_FRONTEND=noninteractive \
    && apt-get -y install --no-install-recommends \
    gstreamer1.0-tools gstreamer1.0-nice \
    gstreamer1.0-plugins-bad gstreamer1.0-plugins-ugly gstreamer1.0-plugins-good \
    libgstreamer1.0-dev libgstreamer-plugins-bad1.0-dev \
    git libglib2.0-dev libsoup2.4-dev libjson-glib-dev \
    build-essential gstreamer1.0-libav \
    && apt-get -y install curl ca-certificates \
    && curl -sS https://sh.rustup.rs | sh -s -- -y --default-toolchain none \
    && ~/.cargo/bin/rustup default nightly

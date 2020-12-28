FROM debian:testing

RUN export DEBIAN_FRONTEND=noninteractive \
  && apt-get -y update \
  && apt-get -y install --no-install-recommends \
  git build-essential \
  curl ca-certificates \
  && curl -sL 'https://deb.nodesource.com/setup_14.x' | bash - \
  && curl -sS 'https://sh.rustup.rs' | sh -s -- -y --default-toolchain none \
  && ~/.cargo/bin/rustup default nightly \
  && apt-get -y update \
  && apt-get -y install --no-install-recommends \
  # compile-time libs
  libglib2.0-dev libgstreamer1.0-dev libgstreamer-plugins-bad1.0-dev \
  nodejs \
  # runtime libs
  gstreamer1.0-libav \
  gstreamer1.0-plugins-good gstreamer1.0-plugins-bad gstreamer1.0-plugins-ugly \
  gstreamer1.0-nice gstreamer1.0-tools

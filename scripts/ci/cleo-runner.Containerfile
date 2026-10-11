FROM docker.io/library/ubuntu:24.04
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential pkg-config git curl ca-certificates unzip python3 time \
    libssl-dev libx11-dev libxi-dev libxrandr-dev libxcursor-dev \
    libgl1-mesa-dev libegl1-mesa-dev libwayland-dev libxkbcommon-dev \
    libicu-dev xvfb xauth util-linux && rm -rf /var/lib/apt/lists/*
RUN useradd --uid 1001 --create-home --shell /bin/bash ferrule-runner \
    && chmod 700 /home/ferrule-runner
RUN mkdir -p /opt/actions-runner /opt/ferrule-runner-tools \
    && curl -fsSL --retry 3 -o /tmp/runner.tar.gz \
      https://github.com/actions/runner/releases/download/v2.337.0/actions-runner-linux-x64-2.337.0.tar.gz \
    && echo '70920811a4f8ad4328818682bca5c6469c1c942fab52448868071d0063816613  /tmp/runner.tar.gz' | sha256sum -c - \
    && tar -xzf /tmp/runner.tar.gz -C /opt/actions-runner \
    && rm /tmp/runner.tar.gz
RUN curl -fsSL --retry 3 -o /tmp/sccache.tar.gz \
      https://github.com/mozilla/sccache/releases/download/v0.18.0/sccache-v0.18.0-x86_64-unknown-linux-musl.tar.gz \
    && echo '45f1447fbe231e3037bde351ef70677dd212216c8d62ae7ca409fecc4d6acc89  /tmp/sccache.tar.gz' | sha256sum -c - \
    && tar -xzf /tmp/sccache.tar.gz -C /tmp \
    && install -m 755 /tmp/sccache-v0.18.0-x86_64-unknown-linux-musl/sccache /opt/ferrule-runner-tools/sccache \
    && rm -rf /tmp/sccache*
COPY guard-ovh-job.sh prepare-ovh-job.py compact-job-evidence.py reclaim-build-cache.py /opt/ferrule-runner-tools/
COPY cleo-runner-entrypoint.sh /usr/local/bin/runner-entrypoint
RUN chmod 755 /opt/ferrule-runner-tools/* /usr/local/bin/runner-entrypoint
ENV HOME=/home/ferrule-runner \
    PATH=/opt/ferrule-runner-tools:/home/ferrule-runner/.cargo/bin:/home/ferrule-runner/.dotnet:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin \
    ACTIONS_RUNNER_HOOK_JOB_STARTED=/opt/ferrule-runner-tools/guard-ovh-job.sh \
    RUNNER_TOOL_CACHE=/home/ferrule-runner/actions-runner/_work/_tool \
    DOTNET_INSTALL_DIR=/home/ferrule-runner/.dotnet \
    TMPDIR=/home/ferrule-runner/ci-tmp
ENTRYPOINT ["/usr/local/bin/runner-entrypoint"]

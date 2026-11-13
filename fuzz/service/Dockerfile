# The continuous-fuzzing service's build, run locally
# (docs/design/continuous-fuzzing.md). The service's own recipe clones this
# repository; this one copies the commit it is built from, which
# `git archive` hands `docker build` as its context, so a local build builds
# the commit under test and nothing else a working tree holds:
#
#   git archive HEAD | docker build -f fuzz/service/Dockerfile -t tinker-pdf-fuzz-service -
#
# The image's `compile` then runs fuzz/service/build.sh as the service runs
# it; fuzz/README.md has the whole local build.
FROM gcr.io/oss-fuzz-base/base-builder-rust
COPY . $SRC/tinker-pdf
WORKDIR $SRC/tinker-pdf
RUN printf '#!/bin/bash -eu\nexec bash "$SRC/tinker-pdf/fuzz/service/build.sh"\n' > $SRC/build.sh

#!/bin/sh
# RUSTC_WRAPPER for jobs that set CARGO_TARGET_DIR. sccache hashes every CARGO_* variable of the compile into its cache key, so a
# build folder that differs per checkout (host runners hash the work folder per job) made every key unique: 2% hits on the Mac
# runner, 0% on Windows, against 95-99% in the containers. The build folder cannot change what rustc compiles, so it is dropped here.
unset CARGO_TARGET_DIR CARGO_BUILD_TARGET_DIR
exec sccache "$@"

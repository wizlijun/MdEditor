#!/bin/sh
# Stand-in for a chatty `ebook-convert`: dumps ~200KB to stderr (more than a
# pipe's OS buffer, ~64KB) before exiting 0, so calibre.rs's run_with_timeout
# test can pin that stdout/stderr are drained concurrently rather than read
# only after exit -- a child writing this much would otherwise block on the
# full pipe forever while run_with_timeout sat waiting for an exit that can
# never come, deadlocking until the timeout.
[ "$1" = "--version" ] && { echo "ebook-convert (calibre 7.0)"; exit 0; }
head -c 200000 /dev/zero | tr '\0' 'x' >&2
cp /dev/null "$2"
exit 0

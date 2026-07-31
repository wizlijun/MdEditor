#!/bin/sh
# Stand-in for a real `ebook-convert` binary, used by calibre.rs tests so the
# suite never depends on Calibre actually being installed on the test host.
[ "$1" = "--version" ] && { echo "ebook-convert (calibre 7.0)"; exit 0; }
cp /dev/null "$2"
exit 0

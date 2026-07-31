#!/bin/sh
# Simulates a wedged `ebook-convert` process so calibre.rs's hand-rolled
# timeout (spawn + poll try_wait) has something to actually time out on.
sleep 60

#!/bin/sh
# Stand-in for `open` (OD_OPEN_COMMAND): logs each URL the app hands to the
# browser to $OD_DATA_DIR/opens.log instead of opening it.
printf '%s %s\n' "$(python3 -c 'import time;print("%.3f"%time.time())')" "$*" >> "${OD_DATA_DIR:?}/opens.log"

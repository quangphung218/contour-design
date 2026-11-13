#!/bin/sh
# put.sh ID NAME TEXT|@FILE : writes one file of project ID on $BASE (overwrite), prints the HTTP code
c=$3
case "$c" in @*) c=$(cat "${c#@}");; esac
python3 -c 'import sys,json;print(json.dumps({"name":sys.argv[1],"content":sys.argv[2],"overwrite":True}))' "$2" "$c" \
  | curl -s -o /dev/null -w '%{http_code}\n' -X POST -H 'content-type: application/json' --data-binary @- "${BASE:?}/api/projects/$1/files"

#!/bin/sh
# mkproj.sh NAME FILE : creates a project on $BASE with FILE as index.html, prints its id
id=$(curl -s -X POST -H 'content-type: application/json' -d "{\"name\":\"$1\"}" "${BASE:?}/api/projects" | python3 -c 'import sys,json;d=json.load(sys.stdin);print((d.get("project") or d)["id"])')
python3 -c 'import sys,json;print(json.dumps({"name":"index.html","content":open(sys.argv[1]).read(),"overwrite":True}))' "$2" \
  | curl -s -o /dev/null -w '%{http_code} ' -X POST -H 'content-type: application/json' --data-binary @- "$BASE/api/projects/$id/files"
echo "$id"

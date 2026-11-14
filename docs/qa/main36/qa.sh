#!/bin/sh
# qa.sh open|opennew [APP]|pids|quit : the QA copy, always with its own HOME
# (so its own data folder) and the mock agents first on PATH.
# $QA_DIR (default: this folder) holds ContourQA.app, home/ (its HOME, with a
# .zprofile that puts $QA_DIR/mock first on PATH), mock/claude (mock-claude
# here) and cg (swiftc -O -o cg cg.swift). QA_REPO: the repo root.
S=${QA_DIR:-$(cd "$(dirname "$0")" && pwd)}
P="$S/mock:/usr/bin:/bin:/usr/sbin:/sbin"
case "$1" in
  open) open --env HOME="$S/home" --env PATH="$P" --env OD_RESCAN_INSTALL_DIRS=0 --env QA_REPO="${QA_REPO:?repo root}" "$S/ContourQA.app" ;;
  opennew) open -n --env HOME="$S/home" --env PATH="$P" --env OD_RESCAN_INSTALL_DIRS=0 --env QA_REPO="${QA_REPO:?repo root}" "$S/${2:-ContourQA.app}" ;;
  pids) pgrep -f "$S/.*\.app/Contents/MacOS/od-desktop" ;;
  quit) osascript -e 'tell application id "dev.contour.desktop.qa" to quit' ;;
esac

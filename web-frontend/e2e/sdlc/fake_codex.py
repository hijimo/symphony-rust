#!/usr/bin/env python3
"""Fake `codex app-server` (JSON-RPC 2.0 over stdio) for the SDLC E2E suite.

Adapted from web-platform/tests/sdlc_support/mod.rs FAKE_CODEX.
Per turn:
  * prompt references `.../docs/changes/N/{intent,spec,plan}.md` -> requests a
    fileChange approval and writes a complete document (keeps frontmatter);
  * prompt contains "[chat]" -> streams a multi-delta reply, no file change.
Every turn streams agent message deltas so the UI shows streaming output.
"""
import json
import os
import re
import sys
import time

LOG = os.environ.get("FAKE_CODEX_LOG", "/tmp/fake-codex-e2e.log")
thread = None
n = 0
rid = 1000


def log(s):
    with open(LOG, "a") as f:
        f.write(s + "\n")


def send(o):
    sys.stdout.write(json.dumps(o) + "\n")
    sys.stdout.flush()


def request(method, params):
    global rid
    rid += 1
    send({"jsonrpc": "2.0", "id": rid, "method": method, "params": params})
    while True:
        line = sys.stdin.readline()
        if not line:
            sys.exit(0)
        m = json.loads(line)
        if m.get("id") == rid and "method" not in m:
            return m.get("result")


DOCS = {
    "intent": "# Intent\n\n## Problem\nusers cannot X\n\n## Proposed outcome\nthey can X\n\n## Affected users and systems\nportal\n\n## Constraints\nnone\n\n## Open questions\nnone\n",
    "spec": "# Spec\n\n## Requirements\nR1 理赔进度可见\n\n## Design\nD1 门户新增进度页\n\n## Concerns\n- PII. owner: sec. resolution: masked\n",
    "plan": "# Plan\n\n## Files\n- src/new.rs\n\n## Steps\n1. do\n\n## Tests\n- t\n\n## Risks\n- r\n\n## Alternatives considered\n- none\n",
}


def keep_fm(path, body):
    fm = ""
    if os.path.exists(path):
        old = open(path).read()
        if old.startswith("---\n"):
            end = old.find("\n---\n", 4)
            if end >= 0:
                fm = old[: end + 5]
    return fm + body


while True:
    line = sys.stdin.readline()
    if not line:
        break
    msg = json.loads(line)
    method = msg.get("method")
    mid = msg.get("id")
    p = msg.get("params") or {}
    if method == "initialize":
        send({"jsonrpc": "2.0", "id": mid, "result": {"userAgent": "fake"}})
    elif method == "initialized":
        pass
    elif method in ("thread/start", "thread/resume"):
        thread = p.get("threadId") or "thread-e2e-1"
        log(method + " " + thread)
        send({"jsonrpc": "2.0", "id": mid, "result": {"thread": {"id": thread}}})
    elif method == "turn/start":
        n += 1
        turn = "turn-%d" % n
        text = "".join(i.get("text", "") for i in p.get("input", []))
        log("turn " + text[:200].replace("\n", " "))
        send({"jsonrpc": "2.0", "id": mid, "result": {"turn": {"id": turn, "status": "inProgress"}}})
        send({"jsonrpc": "2.0", "method": "turn/started", "params": {"threadId": thread, "turn": {"id": turn}}})
        mid_item = "m%d" % n
        chunks = ["收到，", "我来看看", "这份文档。"] if "[chat]" in text else ["working"]
        for c in chunks:
            send({"jsonrpc": "2.0", "method": "item/agentMessage/delta", "params": {"itemId": mid_item, "threadId": thread, "turnId": turn, "delta": c}})
            time.sleep(0.15)
        m = None if "[chat]" in text else re.search(r"`([^`]*docs/changes/\d+)/(intent|spec|plan)\.md`", text)
        if m:
            rel = m.group(1) + "/" + m.group(2) + ".md"
            path = os.path.join(os.getcwd(), rel)
            ch = [{"path": path, "kind": {"type": "update"}, "diff": ""}]
            send({"jsonrpc": "2.0", "method": "item/started", "params": {"threadId": thread, "turnId": turn, "startedAtMs": 0, "item": {"type": "fileChange", "id": "f%d" % n, "status": "inProgress", "changes": ch}}})
            r = request("item/fileChange/requestApproval", {"itemId": "f%d" % n, "threadId": thread, "turnId": turn, "startedAtMs": 0})
            log("file " + rel + " " + json.dumps(r))
            if r and r.get("decision") == "accept":
                open(path, "w").write(keep_fm(path, DOCS[m.group(2)]))
            send({"jsonrpc": "2.0", "method": "item/completed", "params": {"threadId": thread, "turnId": turn, "completedAtMs": 0, "item": {"type": "fileChange", "id": "f%d" % n, "status": "completed", "changes": ch}}})
        send({"jsonrpc": "2.0", "method": "turn/completed", "params": {"threadId": thread, "turn": {"id": turn, "status": "completed"}}})
    elif mid is not None:
        send({"jsonrpc": "2.0", "id": mid, "result": {}})

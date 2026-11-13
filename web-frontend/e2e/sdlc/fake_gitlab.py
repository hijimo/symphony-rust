#!/usr/bin/env python3
"""Fake GitLab for the SDLC E2E suite.

Serves on one port:
  * GitLab REST v4 endpoints used by web-platform (projects, issues, notes,
    merge requests, merge, closes_issues, user, members, related MRs);
  * git smart HTTP (`/{ns}/{repo}.git/...`) through `git http-backend`, backed
    by local bare repositories under GIT_ROOT, so the platform's
    `{platform_host}/{ns}/{repo}.git` clone URL works (clone/fetch/push);
  * `/__state` (GET) and `/__reset` (POST) for test assertions.

Usage: fake_gitlab.py PORT GIT_ROOT
Bare repos are created on demand by `/__seed?ns=..&repo=..` (POST).
"""
import json
import os
import shutil
import subprocess
import sys
import tempfile
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, unquote, urlparse

PORT = int(sys.argv[1])
GIT_ROOT = os.path.abspath(sys.argv[2])
os.makedirs(GIT_ROOT, exist_ok=True)

LOCK = threading.Lock()
STATE = {"issues": {}, "mrs": {}, "merge_tokens": [], "requests": []}
HOST = f"http://127.0.0.1:{PORT}"


def git(args, cwd=None, git_dir=None):
    cmd = ["git"]
    if git_dir:
        cmd += ["--git-dir", git_dir]
    cmd += args
    r = subprocess.run(cmd, cwd=cwd, capture_output=True, text=True)
    if r.returncode != 0:
        raise RuntimeError(f"git {' '.join(args)}: {r.stderr}")
    return r.stdout


def bare_path(ns, repo):
    return os.path.join(GIT_ROOT, ns, f"{repo}.git")


def seed(ns, repo):
    bare = bare_path(ns, repo)
    if os.path.exists(os.path.join(bare, "HEAD")):
        return bare
    os.makedirs(os.path.dirname(bare), exist_ok=True)
    git(["init", "-q", "--bare", "-b", "main", bare])
    git(["config", "http.receivepack", "true"], git_dir=bare)
    tmp = tempfile.mkdtemp()
    try:
        git(["init", "-q", "-b", "main", tmp])
        files = {"README.md": "# app\n", "src/main.rs": "fn main() {}\n", ".gitignore": "target/\n"}
        for p, c in files.items():
            f = os.path.join(tmp, p)
            os.makedirs(os.path.dirname(f), exist_ok=True)
            open(f, "w").write(c)
        git(["add", "-A"], cwd=tmp)
        git(["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "-m", "init"], cwd=tmp)
        git(["push", "-q", bare, "main"], cwd=tmp)
    finally:
        shutil.rmtree(tmp, ignore_errors=True)
    return bare


def user_json():
    return {"id": 1, "username": "alice", "name": "Alice", "avatar_url": None}


def issue_json(ns, repo, i):
    return {
        "id": i["iid"] + 5000, "iid": i["iid"], "title": i["title"], "description": i["description"],
        "state": i["state"], "labels": i["labels"], "author": user_json(), "assignees": [],
        "milestone": None, "created_at": "2026-10-01T00:00:00Z", "updated_at": "2026-10-01T00:00:00Z",
        "closed_at": i.get("closed_at"), "web_url": f"{HOST}/{ns}/{repo}/-/issues/{i['iid']}",
        "user_notes_count": len(i["notes"]),
    }


def mr_json(ns, repo, m):
    return {
        "id": m["iid"] + 1000, "iid": m["iid"], "title": m["title"], "description": m["description"],
        "state": m["state"], "author": user_json(), "source_branch": m["source"], "target_branch": m["target"],
        "reviewers": [], "merge_status": "can_be_merged", "created_at": "2026-10-01T00:00:00Z",
        "updated_at": "2026-10-01T00:00:00Z", "merged_at": m.get("merged_at"),
        "web_url": f"{HOST}/{ns}/{repo}/-/merge_requests/{m['iid']}", "merge_commit_sha": m.get("merge_sha"),
        "head_pipeline": None, "references": {"full": f"{ns}/{repo}!{m['iid']}"},
    }


def split_labels(v):
    if not v:
        return []
    if isinstance(v, list):
        return [x.strip() for x in v if x.strip()]
    return [x.strip() for x in str(v).split(",") if x.strip()]


def real_merge(bare, src, tgt):
    tmp = tempfile.mkdtemp()
    wt = os.path.join(tmp, "wt")
    try:
        git(["worktree", "add", "--detach", wt, tgt], git_dir=bare)
        git(["-c", "user.name=GitLab", "-c", "user.email=gl@local", "merge", "--no-ff", "--no-edit", src], cwd=wt)
        sha = git(["rev-parse", "HEAD"], cwd=wt).strip()
        git(["update-ref", f"refs/heads/{tgt}", sha], git_dir=bare)
        git(["worktree", "remove", "--force", wt], git_dir=bare)
        return sha
    finally:
        shutil.rmtree(tmp, ignore_errors=True)


class H(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, fmt, *args):
        sys.stderr.write("[fake-gitlab] " + (fmt % args) + "\n")

    # ------------------------------------------------------------ helpers
    def body(self):
        n = int(self.headers.get("Content-Length") or 0)
        raw = self.rfile.read(n) if n else b""
        if not raw:
            return {}
        ct = self.headers.get("Content-Type", "")
        if "json" in ct:
            return json.loads(raw)
        return {k: v[0] for k, v in parse_qs(raw.decode()).items()}

    def send_json(self, obj, status=200, headers=None):
        data = json.dumps(obj).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        for k, v in (headers or {}).items():
            self.send_header(k, v)
        self.end_headers()
        self.wfile.write(data)

    def not_found(self, msg="404 Not Found"):
        self.send_json({"message": msg}, 404)

    # ------------------------------------------------------------ git
    def is_git(self, path):
        return ".git/" in path or path.endswith(".git")

    def handle_git(self, method):
        u = urlparse(self.path)
        n = int(self.headers.get("Content-Length") or 0)
        body = self.rfile.read(n) if n else b""
        env = {
            "PATH": os.environ.get("PATH", ""),
            "GIT_PROJECT_ROOT": GIT_ROOT,
            "GIT_HTTP_EXPORT_ALL": "1",
            "REQUEST_METHOD": method,
            "PATH_INFO": unquote(u.path),
            "QUERY_STRING": u.query,
            "CONTENT_TYPE": self.headers.get("Content-Type", ""),
            "CONTENT_LENGTH": str(len(body)),
            "REMOTE_USER": "oauth2",
            "REMOTE_ADDR": "127.0.0.1",
            "GIT_CONFIG_COUNT": "1",
            "GIT_CONFIG_KEY_0": "safe.directory",
            "GIT_CONFIG_VALUE_0": "*",
            "HOME": os.environ.get("HOME", "/tmp"),
        }
        if self.headers.get("Git-Protocol"):
            env["GIT_PROTOCOL"] = self.headers["Git-Protocol"]
        if self.headers.get("Content-Encoding"):
            env["HTTP_CONTENT_ENCODING"] = self.headers["Content-Encoding"]
        r = subprocess.run(["git", "http-backend"], input=body, env=env, capture_output=True)
        out = r.stdout
        sep = out.find(b"\r\n\r\n")
        sep_len = 4
        if sep < 0:
            sep = out.find(b"\n\n")
            sep_len = 2
        if sep < 0:
            self.send_json({"message": "git http-backend failed", "stderr": r.stderr.decode()}, 500)
            return
        head, payload = out[:sep].decode(), out[sep + sep_len:]
        status = 200
        hdrs = []
        for line in head.splitlines():
            if not line.strip():
                continue
            k, _, v = line.partition(":")
            if k.lower() == "status":
                status = int(v.strip().split()[0])
            else:
                hdrs.append((k.strip(), v.strip()))
        self.send_response(status)
        for k, v in hdrs:
            self.send_header(k, v)
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)

    # ------------------------------------------------------------ routing
    def route(self, method):
        u = urlparse(self.path)
        path = u.path
        q = {k: v[0] for k, v in parse_qs(u.query).items()}
        with LOCK:
            STATE["requests"].append(f"{method} {path}")
        if self.is_git(path) and not path.startswith("/api/"):
            return self.handle_git(method)
        if path == "/__state":
            with LOCK:
                return self.send_json({"issues": STATE["issues"], "mrs": STATE["mrs"], "merge_tokens": STATE["merge_tokens"]})
        if path == "/__seed" and method == "POST":
            bare = seed(q["ns"], q["repo"])
            return self.send_json({"bare": bare})
        if path == "/__git":
            # read a file at a ref from a bare repo: ?ns=&repo=&ref=&path=
            try:
                out = git(["show", f"{q['ref']}:{q['path']}"], git_dir=bare_path(q["ns"], q["repo"]))
                return self.send_json({"content": out})
            except RuntimeError as e:
                return self.send_json({"error": str(e)}, 404)
        if path == "/__gitlog":
            try:
                out = git(["log", "--format=%H %P%n%B", "-n", q.get("n", "20"), q["ref"]], git_dir=bare_path(q["ns"], q["repo"]))
                return self.send_json({"log": out})
            except RuntimeError as e:
                return self.send_json({"error": str(e)}, 404)
        if path == "/api/v4/user":
            return self.send_json(user_json())
        if not path.startswith("/api/v4/projects/"):
            return self.not_found()
        rest = path[len("/api/v4/projects/"):]
        parts = rest.split("/")
        proj = unquote(parts[0])
        if "/" not in proj:
            return self.not_found("404 Project Not Found")
        ns, repo = proj.rsplit("/", 1)
        key = f"{ns}/{repo}"
        tail = parts[1:]
        token = self.headers.get("PRIVATE-TOKEN", "")
        if not token:
            return self.send_json({"message": "401 Unauthorized"}, 401)
        b = self.body() if method in ("POST", "PUT") else {}
        with LOCK:
            issues = STATE["issues"].setdefault(key, {})
            mrs = STATE["mrs"].setdefault(key, {})
            # ---------------- project
            if not tail:
                return self.send_json({
                    "id": 1, "path_with_namespace": key, "default_branch": "main",
                    "permissions": {"project_access": {"access_level": 40}, "group_access": None},
                })
            if tail == ["members", "all"]:
                return self.send_json([{"id": 1, "username": "alice", "access_level": 40}])
            # ---------------- issues
            if tail[0] == "issues":
                if len(tail) == 1 and method == "GET":
                    want = q.get("state", "opened")
                    labels = split_labels(q.get("labels"))
                    search = q.get("search")
                    lst = [
                        issue_json(ns, repo, i) for i in sorted(issues.values(), key=lambda x: -x["iid"])
                        if (want == "all" or i["state"] == want)
                        and all(l in i["labels"] for l in labels)
                        and (not search or search.lower() in i["title"].lower())
                    ]
                    return self.send_json(lst, headers={"X-Total": str(len(lst))})
                if len(tail) == 1 and method == "POST":
                    iid = max([int(k) for k in issues] or [0]) + 1
                    issues[str(iid)] = {
                        "iid": iid, "title": b.get("title", ""), "description": b.get("description"),
                        "labels": split_labels(b.get("labels")), "state": "opened", "notes": [],
                    }
                    return self.send_json(issue_json(ns, repo, issues[str(iid)]), 201)
                i = issues.get(tail[1])
                if i is None:
                    return self.not_found("404 Issue Not Found")
                if len(tail) == 2 and method == "GET":
                    return self.send_json(issue_json(ns, repo, i))
                if len(tail) == 2 and method == "PUT":
                    for l in split_labels(b.get("remove_labels")):
                        if l in i["labels"]:
                            i["labels"].remove(l)
                    for l in split_labels(b.get("add_labels")):
                        if l not in i["labels"]:
                            i["labels"].append(l)
                    if "labels" in b:
                        i["labels"] = split_labels(b["labels"])
                    if b.get("state_event") == "close":
                        i["state"] = "closed"
                        i["closed_at"] = "2026-10-27T00:00:00Z"
                    if b.get("state_event") == "reopen":
                        i["state"] = "opened"
                    return self.send_json(issue_json(ns, repo, i))
                if tail[2:] == ["notes"] and method == "POST":
                    i["notes"].append(b.get("body", ""))
                    return self.send_json({"id": len(i["notes"]), "body": b.get("body", "")}, 201)
                if tail[2:] == ["related_merge_requests"]:
                    return self.send_json([])
                return self.not_found()
            # ---------------- merge requests
            if tail[0] == "merge_requests":
                if len(tail) == 1 and method == "GET":
                    want = q.get("state", "opened")
                    lst = [
                        mr_json(ns, repo, m) for m in sorted(mrs.values(), key=lambda x: -x["iid"])
                        if (want == "all" or m["state"] == want)
                        and (not q.get("source_branch") or m["source"] == q["source_branch"])
                        and (not q.get("target_branch") or m["target"] == q["target_branch"])
                    ]
                    return self.send_json(lst, headers={"X-Total": str(len(lst))})
                if len(tail) == 1 and method == "POST":
                    iid = max([int(k) for k in mrs] or [0]) + 1
                    mrs[str(iid)] = {
                        "iid": iid, "title": b.get("title", ""), "description": b.get("description"),
                        "source": b.get("source_branch", ""), "target": b.get("target_branch", ""),
                        "state": "opened", "notes": [], "merge_sha": None,
                    }
                    return self.send_json(mr_json(ns, repo, mrs[str(iid)]), 201)
                m = mrs.get(tail[1])
                if m is None:
                    return self.not_found("404 Merge Request Not Found")
                if len(tail) == 2 and method == "GET":
                    return self.send_json(mr_json(ns, repo, m))
                if len(tail) == 2 and method == "PUT":
                    if "title" in b:
                        m["title"] = b["title"]
                    if b.get("state_event") == "close":
                        m["state"] = "closed"
                    return self.send_json(mr_json(ns, repo, m))
                if tail[2:] == ["notes"] and method == "POST":
                    m["notes"].append(b.get("body", ""))
                    return self.send_json({"id": len(m["notes"])}, 201)
                if tail[2:] == ["closes_issues"]:
                    return self.send_json([])
                if tail[2:] == ["merge"] and method == "PUT":
                    STATE["merge_tokens"].append(token)
                    if b.get("squash") not in (False, "false"):
                        return self.send_json({"message": "docs MR must not be squashed"}, 400)
                    if m["state"] != "opened":
                        return self.send_json({"message": "405 Method Not Allowed"}, 405)
                    if m["title"].startswith("Draft:"):
                        return self.send_json({"message": "draft"}, 405)
                    try:
                        sha = real_merge(bare_path(ns, repo), m["source"], m["target"])
                    except RuntimeError as e:
                        return self.send_json({"message": str(e)}, 405)
                    m["state"] = "merged"
                    m["merge_sha"] = sha
                    m["merged_at"] = "2026-10-27T00:00:00Z"
                    return self.send_json(mr_json(ns, repo, m))
                return self.not_found()
        return self.not_found()

    def do_GET(self):
        self.route("GET")

    def do_POST(self):
        self.route("POST")

    def do_PUT(self):
        self.route("PUT")

    def do_DELETE(self):
        self.route("DELETE")


if __name__ == "__main__":
    srv = ThreadingHTTPServer(("127.0.0.1", PORT), H)
    sys.stderr.write(f"[fake-gitlab] listening on {HOST}, git root {GIT_ROOT}\n")
    srv.serve_forever()

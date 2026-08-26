#!/bin/sh
# Render an mdBook page tree as a GitHub wiki. The wiki is flat: a page in
# `guide/install.md` becomes `guide-install.md`, the book's front page becomes
# `Home.md`, and the summary becomes the sidebar. Every inline link between
# pages is rewritten to the wiki page name it now has; an inline link that
# names no page fails the render, because the wiki would serve it broken.
#
#   wiki.sh <pages-dir> <summary-file> <out-dir>
#
# The pages are the markdown mdBook built (`docs/book/markdown`), not the
# sources, so includes are already expanded. mdBook's markdown renderer emits
# no summary, so the sidebar is rendered from the book's own `SUMMARY.md`.
set -eu

if [ $# -ne 3 ]; then
    echo "usage: wiki.sh <pages-dir> <summary-file> <out-dir>" >&2
    exit 2
fi

pages=$1
summary=$2
out=$3

[ -d "$pages" ] || { echo "wiki.sh: no pages at $pages" >&2; exit 1; }
[ -f "$summary" ] || { echo "wiki.sh: no summary at $summary" >&2; exit 1; }

mkdir -p "$out"

# One awk pass over the summary and every page: the page names are known
# before the first line is read, so a link can be resolved where it is found.
find "$pages" -name '*.md' ! -path "$pages/SUMMARY.md" -exec awk \
    -v root="$pages/" -v summary="$summary" -v out="$out" '
function name_for(path,   name) {
    name = path
    sub(/\.md$/, "", name)
    gsub("/", "-", name)
    return name == "index" ? "Home" : name
}

function claim(name, by,   held) {
    if (name in owner) {
        held = owner[name]
        printf "wiki.sh: %s and %s both want the wiki page name %s\n", \
            held, by, name > "/dev/stderr"
        broken = 1
    }
    owner[name] = by
}

BEGIN {
    # The wiki writes these itself, so no page may answer to them.
    claim("_Sidebar", "the sidebar")
    claim("_Footer", "the footer")

    # ARGV[1] is the summary; the pages follow.
    for (i = 2; i < ARGC; i++) {
        source = substr(ARGV[i], length(root) + 1)
        page[source] = name_for(source)
        claim(page[source], source)
        # An empty page still owns its wiki page.
        printf "" > (out "/" page[source] ".md")
        close(out "/" page[source] ".md")
    }
    if (broken) exit 1
}

# The path a link points at, relative to the root of the page tree.
function resolve(target,   part, count, i, top, stack, joined) {
    count = split((base == "" ? target : base "/" target), part, "/")
    top = 0
    for (i = 1; i <= count; i++) {
        if (part[i] == "" || part[i] == ".") continue
        if (part[i] == "..") {
            if (top == 0) return ""
            top--
            continue
        }
        stack[++top] = part[i]
    }
    joined = ""
    for (i = 1; i <= top; i++) joined = joined "/" stack[i]
    return substr(joined, 2)
}

function rewrite(link,   target, title, anchor, at, path) {
    target = link
    title = ""
    at = index(target, " ")
    if (at > 0) {
        title = substr(target, at)
        target = substr(target, 1, at - 1)
    }
    anchor = ""
    at = index(target, "#")
    if (at > 0) {
        anchor = substr(target, at)
        target = substr(target, 1, at - 1)
    }
    if (target !~ /\.md$/ || target ~ /:\/\//) return link
    path = resolve(target)
    if (!(path in page)) {
        printf "wiki.sh: %s links to %s, which is not a page\n", source, target \
            > "/dev/stderr"
        broken = 1
        return link
    }
    return page[path] anchor title
}

FNR == 1 {
    if (destination != "") close(destination)
    fenced = 0
    if (FILENAME == summary) {
        source = summary
        base = ""
        destination = out "/_Sidebar.md"
    } else {
        source = substr(FILENAME, length(root) + 1)
        base = source
        if (!sub(/\/[^\/]*$/, "", base)) base = ""
        destination = out "/" page[source] ".md"
    }
}

{
    if ($0 ~ /^[ \t]*(```|~~~)/) fenced = !fenced
    if (fenced) { print > destination; next }
    done = ""
    rest = $0
    while (match(rest, /\]\([^)]*\)/)) {
        done = done substr(rest, 1, RSTART) "(" \
               rewrite(substr(rest, RSTART + 2, RLENGTH - 3)) ")"
        rest = substr(rest, RSTART + RLENGTH)
    }
    print done rest > destination
}

END { if (broken) exit 1 }
' "$summary" {} +

cat > "$out/_Footer.md" <<'FOOTER'
Rendered from the book under `docs/`. An edit made here is overwritten by the
next one, so change the book instead.
FOOTER

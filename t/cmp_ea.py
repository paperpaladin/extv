#!/usr/bin/env python3
"""Compare `extv ea` against debugfs ea_list and ea_get.

ea_list gives the names and the lengths in the order they are stored.
ea_get -x gives the value as hex, which is the only comparison worth
making for an attribute value: it is bytes, and debugfs's listing
prints a C string for a short one and nothing at all for a long one.
"""
import re
import subprocess
import sys

EXTV, IMG, NAME, LAST = sys.argv[1], sys.argv[2], sys.argv[3], int(sys.argv[4])


def debugfs(cmds):
    p = subprocess.run(["debugfs", "-f", "/dev/stdin", IMG],
                       input=cmds.encode(), stdout=subprocess.PIPE,
                       stderr=subprocess.DEVNULL)
    return p.stdout.decode("utf-8", "replace")


def main():
    out = debugfs("".join("ea_list <%d>\n" % n for n in range(1, LAST + 1)))
    want = {}
    cur = None
    for line in out.splitlines():
        m = re.match(r"^debugfs:\s+ea_list <(\d+)>", line)
        if m:
            cur = int(m.group(1))
            continue
        m = re.match(r"^  (\S+) \((\d+)\)", line)
        if m and cur is not None:
            want.setdefault(cur, []).append((m.group(1), int(m.group(2))))

    # Now the values, one ea_get per attribute, all in one process.
    cmds = "".join("ea_get -x <%d> %s\n" % (n, a)
                   for n in sorted(want) for a, _ in want[n])
    values = {}
    cur = None
    for line in debugfs(cmds).splitlines():
        m = re.match(r"^debugfs:\s+ea_get -x <(\d+)> (\S+)", line)
        if m:
            cur = (int(m.group(1)), m.group(2))
            values[cur] = ""
            continue
        if cur is None:
            continue
        # The value comes on the line after the echoed command, as
        # "name (len) = xx xx xx", and a long one wraps onto plain hex
        # lines after it.
        m = re.match(r"^\S+ \(\d+\) = (.*)$", line)
        if m:
            values[cur] += m.group(1)
        elif re.match(r"^[0-9a-f ]+$", line):
            values[cur] += line
    for k in values:
        values[k] = "".join(values[k].split())

    p = subprocess.run([EXTV, "ea", IMG, "1-%d" % LAST],
                       stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    if p.returncode != 0:
        print("FAIL %s ea: extv exited %d: %s"
              % (NAME, p.returncode, p.stderr.decode().strip()))
        print("DONE")
        return
    got = {}
    sources = {"inode": 0, "block": 0}
    cur = None
    for line in p.stdout.decode().splitlines():
        f = line.split()
        if f[0] == "inode":
            cur = int(f[1])
        elif f[0] == "attr":
            sources[f[1]] = sources.get(f[1], 0) + 1
            got.setdefault(cur, []).append((f[2], int(f[3]), f[5] if len(f) > 5 else ""))

    bad = []
    prefixes = {}
    nattr = 0
    nbytes = 0
    for n in sorted(set(want) | set(got)):
        w = want.get(n, [])
        g = got.get(n, [])
        if [(a, b) for a, b, _ in g] != w:
            bad.append("inode %d: debugfs=%r extv=%r"
                       % (n, w, [(a, b) for a, b, _ in g]))
            continue
        for (a, ln, hexv) in g:
            nattr += 1
            nbytes += ln
            # Which of the one-byte prefix indices the name came back
            # with. A prefix no attribute in the corpus uses is a row
            # of the table that agrees with anything.
            for p in ("user.", "trusted.", "security.", "system."):
                if a.startswith(p):
                    prefixes[p] = prefixes.get(p, 0) + 1
                    break
            if values.get((n, a), "") != hexv:
                bad.append("inode %d %s value: debugfs=%s... extv=%s..."
                           % (n, a, values.get((n, a), "")[:24], hexv[:24]))
    for b in bad[:8]:
        print("FAIL %s ea %s" % (NAME, b))
    if not bad:
        print("ok   %s ea (%d attributes, %d value bytes, %d in the inode, "
              "%d in a block)" % (NAME, nattr, nbytes, sources.get("inode", 0),
                                  sources.get("block", 0)))
    print("COUNT ea_attrs %d" % nattr)
    print("COUNT ea_value_bytes %d" % nbytes)
    print("COUNT ea_in_inode %d" % sources.get("inode", 0))
    print("COUNT ea_in_block %d" % sources.get("block", 0))
    for p, n in sorted(prefixes.items()):
        print("COUNT ea_prefix_%s %d" % (p.rstrip("."), n))
    print("DONE")


main()

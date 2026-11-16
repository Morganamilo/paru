import os
from pathlib import Path, PurePosixPath
import re
import subprocess
import sys


binary, archive = sys.argv[1:]
dynamic = subprocess.check_output(
    ["readelf", "--dynamic", "--wide", binary],
    env={**os.environ, "LC_ALL": "C"},
    text=True,
)
needed = set(re.findall(r"\(NEEDED\).*\[([^]]+)\]", dynamic))
required = {"libalpm.so.16", "libssl.so.4", "libcrypto.so.4"}
missing = required - needed
if missing:
    sys.exit(f"Missing release dependencies: {', '.join(sorted(missing))}")

unexpected = {
    name
    for name in needed - required
    if name.startswith(("libalpm.so", "libssl.so", "libcrypto.so"))
}
if unexpected:
    sys.exit(f"Unexpected release dependencies: {', '.join(sorted(unexpected))}")
if re.search(r"\((?:RPATH|RUNPATH)\)", dynamic):
    sys.exit("Release binary contains a runtime library search path")

members = subprocess.check_output(["tar", "--zstd", "-tf", archive], text=True)
for member in members.splitlines():
    path = PurePosixPath(member)
    if (
        path.is_absolute()
        or ".." in path.parts
        or path.parts[0] not in {"paru", "paru.conf", "man", "completions", "locale"}
        or path.name.startswith(("libssl.", "libcrypto."))
    ):
        sys.exit(f"Unexpected release archive member: {member}")

archived_binary = subprocess.check_output(["tar", "--zstd", "-xOf", archive, "paru"])
if archived_binary != Path(binary).read_bytes():
    sys.exit("Release archive contains a different binary")

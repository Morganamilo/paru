#!/bin/bash
set -e

if [ -z "$1" ]; then
	echo "usage mkmo <dir>"
	exit 1
fi

for po in po/*.po; do
	lang=$(basename ${po%.po})
	install -dm755 "$1/$lang/LC_MESSAGES/"
	msgfmt "$po" -o "$1/$lang/LC_MESSAGES/paru.mo"
done

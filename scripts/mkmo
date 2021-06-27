#!/bin/sh
set -e

for po in po/*.po; do
	msgfmt "$po" -o "${po%.po}.mo"
done

if [ "$1" == "install" ]; then
	if [ -z "$2" ]; then
	    echo "error: no install dir specified"
	    exit 1
	fi

	for mo in po/*.mo; do
		lang=$(basename ${mo%.mo})
		install -Dm644 "$mo" "$2/$lang/LC_MESSAGES/paru.mo"
	done
fi

#!/bin/sh
set -eu

if [ "$#" -eq 0 ]; then
  exec /bin/bash
fi

# If the first argument is an alchemy subcommand, prepend 'alchemy'
case "$1" in
  tui|rag|check|in|out)
    exec alchemy "$@"
    ;;
  *)
    exec "$@"
    ;;
esac

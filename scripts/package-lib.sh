# The part of the package scripts that each one shares. Source it.
#
# digest <SHA256SUMS> <file>: the digest of one file of the release, or a
# stop that names the file that is missing. SHA256SUMS holds lines of
# "<digest>  <file name>".
digest() {
    local found
    found="$(awk -v f="$2" '$2 == f { print $1 }' "$1")"
    if [[ ! "$found" =~ ^[0-9a-f]{64}$ ]]; then
        echo "$(basename "$0"): $1 holds no digest for $2" >&2
        exit 1
    fi
    echo "$found"
}

# The download address of the files of a release on GitHub.
release_url() {
    echo "https://github.com/Stiven-Gjekaj/OnyxDiskInfo/releases/download/v$1"
}

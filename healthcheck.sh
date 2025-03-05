#!/bin/bash
# A connected stream will receive a ping every 10 seconds. On each ping, the
# program touches ping.txt. If the time since the last ping is more than 30
# seconds, exit with an unhealthy status.

# If ping.txt doesn't exist, container is not yet healthy
if [ ! -f ping.txt ]; then
    echo "ping.txt does not exist yet - waiting for first ping"
    exit 1
fi

current_time=$(date +%s)
# Use the Linux stat format (not macOS)
last_ping_time=$(stat -f %m ping.txt 2>/dev/null || stat -c %Y ping.txt)
time_since_last_ping=$((current_time - last_ping_time))

if [[ $time_since_last_ping -gt 30 ]]; then
    echo "Last ping was $time_since_last_ping seconds ago"
    exit 1
else
    exit 0
fi

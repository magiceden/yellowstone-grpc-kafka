#!/bin/bash
# A connected stream will receive a ping every 10 seconds. On each ping, the
# program touches ping.txt. If the time since the last ping is more than 30
# seconds, exit with an unhealthy status.

# Create ping.txt if it doesn't exist yet
if [ ! -f ping.txt ]; then
  echo "ping" > ping.txt
  exit 0
fi

current_time=$(date +%s)
# Use the Linux stat format (not macOS)
last_ping_time=$(stat -c %Y ping.txt)
time_since_last_ping=$((current_time - last_ping_time))

if [[ $time_since_last_ping -gt 30 ]]; then
  exit 1
else
  exit 0
fi

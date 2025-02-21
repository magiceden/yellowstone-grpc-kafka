#!/bin/bash
# A connected stream will receive a ping every 10 seconds. On each ping, the
# program touches ping.txt. If the time since the last ping is more than 30
# seconds, exit with an unhealthy status.

current_time=$(date +%s)
last_ping_time=$(stat -f %m ping.txt)
time_since_last_ping=$((current_time - last_ping_time))

if [[ $time_since_last_ping -gt 30 ]]; then
  exit 1
else
  exit 0
fi

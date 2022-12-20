#!/bin/bash
# change the socket to the correct socket for the coin that sends walletnotifications there.

# note that in order for the below command to work, `netcat-openbsd` needs to be installed. Sometimes the wrong netcat version is installed at default.
echo $1 | nc -U -q 5 /tmp/discord_bot_vrsc.sock &

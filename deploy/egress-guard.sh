#!/bin/sh
# Run as root after Docker starts. Only the dedicated GG bridge is affected.
set -eu
iptables -w -N GG-RETRO-EGRESS 2>/dev/null || true
iptables -w -C GG-RETRO-EGRESS -m conntrack --ctstate ESTABLISHED,RELATED -j RETURN 2>/dev/null || \
  iptables -w -I GG-RETRO-EGRESS 1 -m conntrack --ctstate ESTABLISHED,RELATED -j RETURN
iptables -w -C GG-RETRO-EGRESS -j REJECT 2>/dev/null || iptables -w -A GG-RETRO-EGRESS -j REJECT
iptables -w -C DOCKER-USER -i br-gg-retro -j GG-RETRO-EGRESS 2>/dev/null || \
  iptables -w -I DOCKER-USER 1 -i br-gg-retro -j GG-RETRO-EGRESS
iptables -w -C INPUT -i br-gg-retro -m conntrack --ctstate NEW -j REJECT 2>/dev/null || \
  iptables -w -I INPUT 1 -i br-gg-retro -m conntrack --ctstate NEW -j REJECT

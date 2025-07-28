#!/usr/bin/env python3

import scapy.all as scapy

packet = open("icmp-packet", "rb").read()

print(scapy.IP(packet).show())

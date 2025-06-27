import requests as r
import os

base_url = "http://localhost:3000"
#
# r.post(f"{base_url}/order", json={
#   "price": 122,
#   "amount": 4,
#   "type": 1,
#   "side": 2
# })
#
# r.post(f"{base_url}/order", json={
#   "price": 122,
#   "amount": 4,
#   "type": 1,
#   "side": 1
# })
#
# orderbook = r.get(f"{base_url}/orderbooks")
#
# print(orderbook.text)

counter = 0;

while True:
    _ = r.get(base_url)
    counter += 1
    print(counter)


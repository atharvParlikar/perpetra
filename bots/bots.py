import requests
import random
import json
import time

API_URL = "http://localhost:3000/order"
HEADERS = {"Content-Type": "application/json"}

USERNAMES = ["alice", "bob", "charlie", "dave"]
ORDER_TYPES = ["market", "limit"]
ORDER_SIDES = ["buy", "sell"]

for i in range(10):
    payload = {
        "type_": random.choice(ORDER_TYPES),
        "amount": round(random.uniform(1.0, 100.0), 2),
        "price": round(random.uniform(59000.0, 61000.0), 2),
        "side": random.choice(ORDER_SIDES),
        "jwt": random.choice(USERNAMES)
    }

    print(f"\nSending Order {i+1}:")
    print(json.dumps(payload, indent=2))

    try:
        response = requests.post(API_URL, json=payload, headers=HEADERS)
        print("Response:", response.status_code, response.text)
    except Exception as e:
        print("Error sending request:", e)

    time.sleep(0.2)

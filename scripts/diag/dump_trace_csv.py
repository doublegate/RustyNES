import csv
with open('trace.csv') as f:
    reader = csv.DictReader(f)
    last_addr = None
    count = 0
    for row in reader:
        print(row)
        count += 1
        if count > 10: break

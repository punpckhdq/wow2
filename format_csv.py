#!/usr/bin/env python3
import csv

with open('modules.csv', 'r') as csvfile:
    reader = csv.reader(csvfile)
    with open('src/data/modules.txt', 'w') as txtfile:
        print('&[', file=txtfile)
        for row in reader:
            print(f'Module {{ index: {row[0]}, path: "{row[1]}" }},', file=txtfile)
        print(']', file=txtfile)

with open('symbols.csv', 'r') as csvfile:
    reader = csv.reader(csvfile)
    list = [row for row in reader]
    list.sort(key=lambda x: int(x[0]))
    with open('src/data/symbols.txt', 'w') as txtfile:
        print('&[', file=txtfile)
        for row in list:
            print(f'Symbol {{ file_offset: {row[0]}, name: "{row[1]}" }},', file=txtfile)
        print(']', file=txtfile)

with open('contribs.csv', 'r') as csvfile:
    reader = csv.reader(csvfile)
    with open('src/data/contribs.txt', 'w') as txtfile:
        print('&[', file=txtfile)
        for row in reader:
            print(f'Contrib {{ file_offset: {row[0]}, size: {row[1]}, characteristics: {row[2]}, module_index: {row[3]} }},', file=txtfile)
        print(']', file=txtfile)

#with open('sections.csv', 'r') as csvfile:
#    reader = csv.reader(csvfile)
#    with open('src/sections.txt', 'w') as txtfile:
#        for row in reader:
#            print(f'Section {{ offset: {row[0]}, size: {row[1]}, name: {row[2]}, flags: {row[3]} }},', file=txtfile)

with open('splits.csv', 'r') as csvfile:
    reader = csv.reader(csvfile)
    with open('src/data/splits.txt', 'w') as txtfile:
        print('&[', file=txtfile)
        for row in reader:
            print(f'Split {{ file_offset: {row[0]}, size: {row[1]}, name: "{row[2]}", flags: {row[3]} }},', file=txtfile)
        print(']', file=txtfile)

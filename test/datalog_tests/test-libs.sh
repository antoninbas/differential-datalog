#!/bin/bash

# Test DDlog libraries. 

set -e

./run-test.sh lib_test.dl release

# $1 - test name
test_lib() {
    echo Running $1 test
    /usr/bin/time ./lib_test_ddlog/target/release/lib_test_cli --no-print < $1.dat > $1.dump
    diff -q $1.dump.expected $1.dump
}

test_lib uuid_test
test_lib net_test
test_lib json_test

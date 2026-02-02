CC       = gcc
CFLAGS   = -Wall -Wextra -O2 $(shell pkg-config --cflags pappl)
LDFLAGS  = $(shell pkg-config --libs pappl)
RUST_LIB = ../target/release/libkatasymbol_ffi.a
RUST_DEPS= -ldbus-1 -llzma -lbluetooth -lpthread -lm -ldl
TARGET   = katasymbol-printer-app

all: rust $(TARGET)

rust:
	cd .. && cargo build --release -p katasymbol-ffi

$(TARGET): katasymbol-printer-app.o $(RUST_LIB)
	$(CC) -o $@ $< $(RUST_LIB) $(LDFLAGS) $(RUST_DEPS)

%.o: %.c
	$(CC) $(CFLAGS) -c -o $@ $<

clean:
	rm -f $(TARGET) *.o

.PHONY: all rust clean

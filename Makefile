CARGO = cargo
FLAGS = --all-features

.PHONY: test help clean run

all: test

test:
	@echo "Running all unit tests..."
	$(CARGO) test $(FLAGS)

help:
	@echo "Usage: make [target]"
	@echo ""
	@echo "Targets:"
	@sed -n 's/^##//p' $(MAKEFILE_LIST) | column -t -s ':' |  sed -e 's/^/ /'

clean:
	$(CARGO) clean

run:
	$(CARGO) run

fmt:
	$(CARGO) +nightly fmt

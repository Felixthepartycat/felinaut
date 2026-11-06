.PHONY: dist

# Always rebuild the optimized app before creating the shareable installer.
dist:
	cargo dist

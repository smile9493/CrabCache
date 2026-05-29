.PHONY: help hot-update hot-update-legacy build-dashboard

help:
	@echo "Available targets:"
	@echo "  make hot-update           # Python: local build + push to remote containers (see AGENTS.md)"
	@echo "  make hot-update-legacy    # Bash hot_update_runtime.sh (local DOCKER_HOST required)"
	@echo "  make build-dashboard      # Build crab-dashboard static dist assets"

hot-update:
	@python3 scripts/hot_update.py

hot-update-legacy:
	@./scripts/hot_update_runtime.sh

build-dashboard:
	@./scripts/build_dashboard.sh

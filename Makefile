.PHONY: help hot-update build-dashboard

help:
	@echo "Available targets:"
	@echo "  make hot-update      # Rebuild gateway/admin + dashboard dist and hot update containers"
	@echo "  make build-dashboard # Build crab-dashboard static dist assets"

hot-update:
	@./scripts/hot_update_runtime.sh

build-dashboard:
	@./scripts/build_dashboard.sh

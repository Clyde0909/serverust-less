/**
 * Packages Component
 * Handles displaying and managing packages in main venv
 */

const Packages = {
    container: null,
    packages: [],
    
    init() {
        this.container = document.getElementById('main-venv-packages');
    },
    
    async load() {
        this.init();
        
        const infoCard = document.getElementById('main-venv-info');
        if (infoCard) {
            infoCard.innerHTML = `
                <div class="venv-status">
                    <div class="spinner" style="width: 16px; height: 16px;"></div>
                    <span>Loading packages...</span>
                </div>
            `;
        }
        
        try {
            const response = await API.Packages.list();
            const packages = response.packages || response;
            Logger.info('Loaded', packages.length, 'packages');
            this.packages = packages;
            AppState.packages = packages;
            this.render(packages);
        } catch (error) {
            Toast.error('Failed to load packages', error.message);
            if (infoCard) {
                infoCard.innerHTML = `
                    <div class="venv-status">
                        <span class="status-indicator inactive"></span>
                        <span>Failed to load packages</span>
                    </div>
                `;
            }
        }
    },
    
    render(packages) {
        this.init();
        
        const infoCard = document.getElementById('main-venv-info');
        if (infoCard) {
            infoCard.innerHTML = `
                <div class="venv-status">
                    <span class="status-indicator"></span>
                    <span>Main Virtual Environment</span>
                    <span class="badge badge-info" style="margin-left: auto;">${packages.length} packages</span>
                </div>
            `;
        }
        
        if (!packages || packages.length === 0) {
            this.container.innerHTML = `
                <div class="empty-state" style="padding: var(--spacing-lg);">
                    <span class="empty-icon">📦</span>
                    <p>No packages installed in main venv</p>
                </div>
            `;
            return;
        }
        
        this.container.innerHTML = packages.map(pkg => this.renderPackageItem(pkg)).join('');
        
        // Attach event listeners
        this.container.querySelectorAll('[data-action="uninstall"]').forEach(btn => {
            btn.addEventListener('click', async (e) => {
                const name = e.currentTarget.dataset.name;
                await this.uninstallPackage(name);
            });
        });
    },
    
    renderPackageItem(pkg) {
        // Handle both object and string formats
        const name = typeof pkg === 'string' ? pkg : (pkg.name || pkg);
        const version = typeof pkg === 'object' ? pkg.version : null;
        
        return `
            <div class="package-item">
                <div>
                    <span class="package-name">${escapeHtml(name)}</span>
                    ${version ? `<span class="package-version">${escapeHtml(version)}</span>` : ''}
                </div>
                <button class="btn btn-sm btn-secondary" data-action="uninstall" data-name="${escapeHtml(name)}" title="Uninstall">
                    🗑️
                </button>
            </div>
        `;
    },
    
    async uninstallPackage(name) {
        if (!confirm(`Are you sure you want to uninstall "${name}"?`)) {
            return;
        }
        
        try {
            await API.Packages.uninstall(name);
            Toast.success('Uninstalled', `Package "${name}" has been uninstalled`);
            this.load();
        } catch (error) {
            Toast.error('Failed to uninstall', error.message);
        }
    },
    
    async searchPackages(query) {
        if (!query || query.length < 2) {
            return [];
        }
        
        try {
            return await API.Packages.search(query);
        } catch (error) {
            console.error('Package search failed:', error);
            return [];
        }
    },
};

/**
 * Packages Component
 * Handles displaying and managing packages in a selected virtual environment
 */

const Packages = {
    container: null,
    packages: [],
    selectedVenv: null,   // { id, name, isMain }
    _selectSetup: false,

    init() {
        this.container = document.getElementById('main-venv-packages');
        if (!this._selectSetup) {
            this._setupSelect();
            this._selectSetup = true;
        }
    },

    _setupSelect() {
        const sel = document.getElementById('packages-venv-select');
        if (!sel) return;
        sel.addEventListener('change', async () => {
            const opt = sel.options[sel.selectedIndex];
            if (!opt || !opt.value) return;
            this.selectedVenv = {
                id: opt.value,
                name: opt.text,
                isMain: opt.dataset.isMain === 'true',
            };
            await this._loadPackages();
        });
    },

    _venvDisplayName(venv) {
        if (venv.venv_type === 'main') return 'main (Default)';
        const parts = (venv.path || '').replace(/\\/g, '/').split('/');
        return parts[parts.length - 1] || venv.id;
    },

    async load() {
        this.init();

        const infoCard = document.getElementById('main-venv-info');
        if (infoCard) {
            infoCard.innerHTML = `
                <div class="venv-status">
                    <div class="spinner" style="width: 16px; height: 16px;"></div>
                    <span>Loading environments...</span>
                </div>
            `;
        }

        // Populate venv dropdown
        try {
            const response = await API.Venvs.list();
            const venvs = response.venvs || response;
            const sel = document.getElementById('packages-venv-select');
            if (sel) {
                sel.innerHTML = venvs.map(v => {
                    const name = this._venvDisplayName(v);
                    const isMain = v.venv_type === 'main';
                    return `<option value="${escapeHtml(v.id)}" data-is-main="${isMain}">${escapeHtml(name)}</option>`;
                }).join('');

                // Default to main venv
                const mainVenv = venvs.find(v => v.venv_type === 'main') || venvs[0];
                if (mainVenv) {
                    sel.value = mainVenv.id;
                    this.selectedVenv = {
                        id: mainVenv.id,
                        name: this._venvDisplayName(mainVenv),
                        isMain: mainVenv.venv_type === 'main',
                    };
                }
            }
        } catch (error) {
            Logger.error('Failed to load venv list for packages:', error);
        }

        await this._loadPackages();
    },

    async _loadPackages() {
        this.init();
        const infoCard = document.getElementById('main-venv-info');

        if (!this.selectedVenv) {
            if (infoCard) infoCard.innerHTML = `<div class="venv-status"><span>No environment selected</span></div>`;
            if (this.container) this.container.innerHTML = '';
            return;
        }

        if (infoCard) {
            infoCard.innerHTML = `
                <div class="venv-status">
                    <div class="spinner" style="width: 16px; height: 16px;"></div>
                    <span>Loading packages for "${escapeHtml(this.selectedVenv.name)}"...</span>
                </div>
            `;
        }

        try {
            let packages;
            if (this.selectedVenv.isMain) {
                // Main venv: use existing packages API (DB-backed)
                const response = await API.Packages.list();
                packages = response.packages || response;
            } else {
                // Custom venv: use pip list via new endpoint
                const response = await API.Venvs.listPackages(this.selectedVenv.id);
                packages = (response.packages || []).map(p => ({
                    package_name: p.name,
                    version: p.version,
                    status: 'ready',
                }));
            }
            Logger.info('Loaded', packages.length, 'packages for', this.selectedVenv.name);
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
        const venvName = this.selectedVenv ? this.selectedVenv.name : 'Unknown';
        if (infoCard) {
            infoCard.innerHTML = `
                <div class="venv-status">
                    <span class="status-indicator"></span>
                    <span>${escapeHtml(venvName)}</span>
                    <span class="badge badge-info" style="margin-left: auto;">${packages.length} packages</span>
                </div>
            `;
        }

        if (!packages || packages.length === 0) {
            this.container.innerHTML = `
                <div class="empty-state" style="padding: var(--spacing-lg);">
                    <span class="empty-icon">📦</span>
                    <p>No packages installed in this environment</p>
                </div>
            `;
            return;
        }

        this.container.innerHTML = packages.map(pkg => this.renderPackageItem(pkg)).join('');

        // Attach uninstall listeners only for main venv (custom venv uninstall not yet supported)
        if (this.selectedVenv?.isMain) {
            this.container.querySelectorAll('[data-action="uninstall"]').forEach(btn => {
                btn.addEventListener('click', async (e) => {
                    const name = e.currentTarget.dataset.name;
                    await this.uninstallPackage(name);
                });
            });
        }
    },

    renderPackageItem(pkg) {
        const name = typeof pkg === 'string' ? pkg : (pkg.package_name || pkg.name || pkg);
        const version = typeof pkg === 'object' ? (pkg.version || null) : null;
        const status = typeof pkg === 'object' ? pkg.status : null;
        const canUninstall = this.selectedVenv?.isMain;

        return `
            <div class="package-item">
                <div>
                    <span class="package-name">${escapeHtml(name)}</span>
                    ${version ? `<span class="package-version">${escapeHtml(version)}</span>` : ''}
                    ${status ? `<span class="badge badge-${status === 'ready' ? 'success' : status === 'installing' ? 'warning' : 'danger'}">${status}</span>` : ''}
                </div>
                ${canUninstall ? `
                <button class="btn btn-sm btn-secondary" data-action="uninstall" data-name="${escapeHtml(name)}" title="Uninstall">
                    🗑️
                </button>` : ''}
            </div>
        `;
    },

    async uninstallPackage(name) {
        const ok = await Confirm.show(`Are you sure you want to uninstall "${name}"?`, { title: 'Uninstall Package', confirmText: 'Uninstall' });
        if (!ok) return;

        try {
            await API.Packages.uninstall(name);
            Toast.success('Uninstalled', `Package "${name}" has been uninstalled`);
            await this._loadPackages();
        } catch (error) {
            Toast.error('Failed to uninstall', error.message);
        }
    },

    async searchPackages(query) {
        if (!query || query.length < 2) {
            return [];
        }

        try {
            const response = await API.Packages.search(query);
            return response.results || [];
        } catch (error) {
            console.error('Package search failed:', error);
            return [];
        }
    },
};

/**
 * Venvs Component
 * Handles displaying and managing virtual environments
 */

const Venvs = {
    container: null,
    emptyState: null,
    venvs: [],
    
    init() {
        this.container = document.getElementById('venvs-list');
        this.emptyState = document.getElementById('venvs-empty');
    },
    
    async load() {
        this.init();
        
        this.container.innerHTML = `
            <div class="loading-state" style="grid-column: 1 / -1;">
                <div class="spinner"></div>
                <p>Loading virtual environments...</p>
            </div>
        `;
        this.emptyState?.classList.add('hidden');
        
        try {
            const response = await API.Venvs.list();
            const venvs = response.venvs || response;
            Logger.info('Loaded', venvs.length, 'venvs');
            this.venvs = venvs;
            AppState.venvs = venvs;
            this.render(venvs);
        } catch (error) {
            Toast.error('Failed to load venvs', error.message);
            this.container.innerHTML = '';
            this.emptyState?.classList.remove('hidden');
        }
    },
    
    render(venvs) {
        this.init();
        
        if (!venvs || venvs.length === 0) {
            this.container.innerHTML = '';
            this.emptyState?.classList.remove('hidden');
            return;
        }
        
        this.emptyState?.classList.add('hidden');
        this.container.innerHTML = venvs.map(venv => this.renderVenvCard(venv)).join('');
        
        // Attach event listeners
        this.container.querySelectorAll('[data-action]').forEach(btn => {
            btn.addEventListener('click', async (e) => {
                const action = e.currentTarget.dataset.action;
                const venvId = e.currentTarget.dataset.venvId;
                await this.handleAction(action, venvId);
            });
        });
    },
    
    renderVenvCard(venv) {
        const isMain = venv.name === 'main' || venv.is_main;
        const statusClass = venv.status === 'ready' ? 'badge-success' : 
                           venv.status === 'creating' ? 'badge-warning' : 'badge-secondary';
        
        return `
            <div class="card venv-card" data-venv-id="${venv.id}">
                <div class="card-header">
                    <h3 class="card-title">
                        🐍 ${escapeHtml(venv.name || venv.id)}
                    </h3>
                    <span class="badge ${statusClass}">${venv.status || 'unknown'}</span>
                </div>
                <div class="card-meta">
                    ${isMain ? '<span class="badge badge-info">Main Venv</span>' : '<span class="badge badge-warning">Custom</span>'}
                    ${venv.python_version ? `<span class="badge badge-secondary">Python ${escapeHtml(venv.python_version)}</span>` : ''}
                </div>
                ${venv.job_id ? `
                    <p style="font-size: 0.875rem; color: var(--text-secondary); margin-bottom: var(--spacing-sm);">
                        Job: ${escapeHtml(venv.job_id)}
                    </p>
                ` : ''}
                ${venv.packages && venv.packages.length > 0 ? `
                    <div class="venv-packages">
                        ${venv.packages.slice(0, 5).map(pkg => {
                            const name = typeof pkg === 'string' ? pkg : pkg.name;
                            return `<span class="badge badge-secondary">${escapeHtml(name)}</span>`;
                        }).join('')}
                        ${venv.packages.length > 5 ? `<span class="badge badge-secondary">+${venv.packages.length - 5} more</span>` : ''}
                    </div>
                ` : ''}
                <div class="card-actions">
                    <button class="btn btn-secondary btn-sm" data-action="view" data-venv-id="${venv.id}">
                        <span class="icon">👁️</span> View Packages
                    </button>
                    ${!isMain ? `
                        <button class="btn btn-danger btn-sm" data-action="delete" data-venv-id="${venv.id}">
                            <span class="icon">🗑️</span> Delete
                        </button>
                    ` : ''}
                </div>
            </div>
        `;
    },
    
    async handleAction(action, venvId) {
        switch (action) {
            case 'view':
                await this.viewPackages(venvId);
                break;
            case 'delete':
                await this.deleteVenv(venvId);
                break;
        }
    },
    
    async viewPackages(venvId) {
        try {
            const packages = await API.Venvs.packages(venvId);
            const venv = this.venvs.find(v => v.id === venvId);
            
            // Show in a simple alert for now - could be enhanced with a modal
            const pkgList = packages.map(p => {
                const name = typeof p === 'string' ? p : p.name;
                const version = typeof p === 'object' ? p.version : '';
                return `${name}${version ? ` (${version})` : ''}`;
            }).join('\n');
            
            alert(`Packages in ${venv?.name || venvId}:\n\n${pkgList || '(no packages)'}`);
        } catch (error) {
            Toast.error('Failed to load packages', error.message);
        }
    },
    
    async deleteVenv(venvId) {
        const venv = this.venvs.find(v => v.id === venvId);
        if (!venv) return;
        
        if (!confirm(`Are you sure you want to delete "${venv.name || venvId}"?\n\nThis will remove the virtual environment and all its packages.`)) {
            return;
        }
        
        try {
            await API.Venvs.delete(venvId);
            Toast.success('Deleted', `Virtual environment has been deleted`);
            this.load();
        } catch (error) {
            Toast.error('Failed to delete', error.message);
        }
    },
};

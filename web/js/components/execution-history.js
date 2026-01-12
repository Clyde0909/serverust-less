/**
 * Execution History Component
 * Handles displaying and managing execution history
 */

const ExecutionHistory = {
    container: null,
    tbody: null,
    emptyState: null,
    loadingState: null,
    allExecutions: [],
    currentFilter: '',
    activeStreams: new Map(),
    
    init() {
        this.container = document.getElementById('executions-list');
        this.tbody = document.getElementById('executions-tbody');
        this.emptyState = document.getElementById('executions-empty');
        this.loadingState = document.getElementById('executions-loading');
    },
    
    async load(silent = false) {
        this.init();
        Logger.debug('ExecutionHistory.load called, silent:', silent);
        
        if (!silent) {
            this.tbody.innerHTML = '';
            this.loadingState?.classList.remove('hidden');
            this.emptyState?.classList.add('hidden');
        }
        
        try {
            const params = { limit: 100 };
            if (this.currentFilter) {
                params.status = this.currentFilter;
            }
            
            const response = await API.Executions.list(params);
            const executions = response.executions || response;
            Logger.info('Loaded', executions.length, 'executions');
            this.allExecutions = executions;
            AppState.executions = executions;
            this.render(executions);
        } catch (error) {
            Logger.error('Failed to load executions:', error);
            Toast.error('Failed to load executions', error.message);
            this.emptyState?.classList.remove('hidden');
        } finally {
            this.loadingState?.classList.add('hidden');
        }
    },
    
    filterByStatus(status) {
        this.currentFilter = status;
        this.load();
    },
    
    render(executions) {
        this.init();
        
        if (!executions || executions.length === 0) {
            this.tbody.innerHTML = '';
            this.emptyState?.classList.remove('hidden');
            this.container?.classList.add('hidden');
            return;
        }
        
        this.emptyState?.classList.add('hidden');
        this.container?.classList.remove('hidden');
        
        this.tbody.innerHTML = executions.map(exec => this.renderRow(exec)).join('');
        
        // Attach event listeners
        this.tbody.querySelectorAll('[data-action]').forEach(btn => {
            btn.addEventListener('click', (e) => {
                const action = e.currentTarget.dataset.action;
                const execId = e.currentTarget.dataset.execId;
                this.handleAction(action, execId);
            });
        });
        
        // Start streaming for running executions
        executions
            .filter(e => e.status === 'running' || e.status === 'pending' || e.status === 'queued')
            .forEach(e => this.startStreaming(e.id));
    },
    
    renderRow(exec) {
        const job = AppState.jobs.find(j => j.id === exec.job_id);
        const jobName = job?.name || exec.job_id;
        
        const duration = formatDuration(exec.started_at, exec.completed_at);
        const isRunning = exec.status === 'running' || exec.status === 'pending' || exec.status === 'queued';
        
        return `
            <tr data-exec-id="${exec.id}">
                <td>
                    <code style="font-size: 0.75rem;">${exec.id.substring(0, 8)}...</code>
                </td>
                <td>${escapeHtml(jobName)}</td>
                <td>
                    <span class="${getStatusBadgeClass(exec.status)}">
                        ${exec.status}
                    </span>
                </td>
                <td>${duration}</td>
                <td>${formatDate(exec.started_at)}</td>
                <td>
                    <div style="display: flex; gap: 4px;">
                        <button class="btn btn-secondary btn-sm" data-action="view" data-exec-id="${exec.id}">
                            View
                        </button>
                        ${isRunning ? `
                            <button class="btn btn-danger btn-sm" data-action="cancel" data-exec-id="${exec.id}">
                                Cancel
                            </button>
                        ` : ''}
                    </div>
                </td>
            </tr>
        `;
    },
    
    async handleAction(action, execId) {
        switch (action) {
            case 'view':
                await this.viewExecution(execId);
                break;
            case 'cancel':
                await this.cancelExecution(execId);
                break;
        }
    },
    
    async viewExecution(execId) {
        try {
            const exec = await API.Executions.get(execId);
            this.showExecutionModal(exec);
        } catch (error) {
            Toast.error('Failed to load execution', error.message);
        }
    },
    
    showExecutionModal(exec) {
        AppState.selectedExecution = exec;
        
        const job = AppState.jobs.find(j => j.id === exec.job_id);
        const jobName = job?.name || exec.job_id;
        
        document.getElementById('exec-id').textContent = exec.id;
        document.getElementById('exec-job-id').textContent = jobName;
        document.getElementById('exec-status').innerHTML = `<span class="${getStatusBadgeClass(exec.status)}">${exec.status}</span>`;
        document.getElementById('exec-duration').textContent = formatDuration(exec.started_at, exec.completed_at);
        document.getElementById('exec-started').textContent = formatDate(exec.started_at);
        
        // Display output data and error message (trim trailing newlines)
        const outText = (exec.output_data || '').replace(/\n+$/,'');
        const errText = (exec.error_message || '').replace(/\n+$/,'');
        document.getElementById('exec-stdout').textContent = outText || '(no output)';
        document.getElementById('exec-stderr').textContent = errText || '(no errors)';
        
        // Load logs
        this.loadExecutionLogs(exec.id);
        
        // Show/hide action buttons based on status
        const isRunning = exec.status === 'running' || exec.status === 'pending' || exec.status === 'queued';
        const cancelBtn = document.getElementById('btn-cancel-execution');
        const retryBtn = document.getElementById('btn-retry-execution');
        
        cancelBtn?.classList.toggle('hidden', !isRunning);
        retryBtn?.classList.toggle('hidden', isRunning);
        
        // Setup cancel button
        if (cancelBtn) {
            cancelBtn.onclick = async () => {
                await this.cancelExecution(exec.id);
                Modal.close('execution-modal');
            };
        }
        
        // Setup retry button
        if (retryBtn) {
            retryBtn.onclick = async () => {
                try {
                    await API.Jobs.execute(exec.job_id);
                    Toast.success('Job re-queued', 'Execution has been queued');
                    Modal.close('execution-modal');
                    this.load();
                } catch (error) {
                    Toast.error('Failed to retry', error.message);
                }
            };
        }
        
        // Reset tabs to output
        document.querySelectorAll('#execution-modal .tab-btn').forEach(btn => {
            btn.classList.toggle('active', btn.dataset.tab === 'stdout');
        });
        document.querySelectorAll('#execution-modal .tab-content').forEach(content => {
            content.classList.toggle('active', content.id === 'exec-stdout');
        });
        
        Modal.open('execution-modal');
        
        // Start streaming if running
        if (isRunning) {
            this.startModalStreaming(exec.id);
        }
    },
    
    async loadExecutionLogs(execId) {
        const logsContainer = document.getElementById('exec-logs');
        if (!logsContainer) return;
        
        try {
            const response = await API.Executions.getLogs(execId, { limit: 1000 });
            const logs = response.logs || response;
            Logger.debug('Loaded', logs.length, 'execution logs');
            
            if (!logs || logs.length === 0) {
                logsContainer.innerHTML = '<span style="color: var(--text-muted);">(no logs)</span>';
                return;
            }
            
            logsContainer.innerHTML = logs.map(log => {
                const content = (log.log_content || '').replace(/\n+$/,'');
                return `<div class="log-entry ${log.log_type || 'stdout'}"><span style="color: var(--text-muted);">[${formatDate(log.created_at)}]</span> ${escapeHtml(content)}</div>`;
            }).join('');
            
            // Scroll to bottom
            logsContainer.scrollTop = logsContainer.scrollHeight;
        } catch (error) {
            Logger.error('Failed to load execution logs:', error);
            logsContainer.innerHTML = '<span style="color: var(--color-danger);">Failed to load logs</span>';
        }
    },
    
    async cancelExecution(execId) {
        if (!confirm('Are you sure you want to cancel this execution?')) {
            return;
        }
        
        try {
            await API.Executions.cancel(execId);
            Toast.success('Cancelled', 'Execution has been cancelled');
            this.stopStreaming(execId);
            this.load();
        } catch (error) {
            Toast.error('Failed to cancel', error.message);
        }
    },
    
    async clearCompleted() {
        Toast.info('Not implemented', 'This feature is not yet available');
    },
    
    // SSE Streaming for real-time updates
    startStreaming(execId) {
        if (this.activeStreams.has(execId)) {
            return; // Already streaming
        }
        
        try {
            const eventSource = API.Executions.stream(execId);
            this.activeStreams.set(execId, eventSource);
            
            eventSource.onmessage = (event) => {
                try {
                    const data = JSON.parse(event.data);
                    this.handleStreamEvent(execId, data);
                } catch (e) {
                    console.error('Failed to parse stream event:', e);
                }
            };
            
            eventSource.onerror = () => {
                this.stopStreaming(execId);
            };
            
            eventSource.addEventListener('complete', () => {
                this.stopStreaming(execId);
                this.load(true); // Refresh silently
            });
        } catch (error) {
            console.error('Failed to start streaming:', error);
        }
    },
    
    stopStreaming(execId) {
        const eventSource = this.activeStreams.get(execId);
        if (eventSource) {
            eventSource.close();
            this.activeStreams.delete(execId);
        }
    },
    
    startModalStreaming(execId) {
        const eventSource = API.Executions.stream(execId);
        
        eventSource.addEventListener('log', (event) => {
            try {
                const data = JSON.parse(event.data);
                const logsContainer = document.getElementById('exec-logs');
                const stdoutEl = document.getElementById('exec-stdout');
                const stderrEl = document.getElementById('exec-stderr');
                
                // Update stdout/stderr tabs based on log_type (trim trailing newlines)
                const logContent = (data.content || '').replace(/\n+$/,'');
                if (data.log_type === 'stdout' && stdoutEl) {
                    if (stdoutEl.textContent === '(no output)') {
                        stdoutEl.textContent = '';
                    }
                    stdoutEl.textContent += logContent + '\n';
                } else if (data.log_type === 'stderr' && stderrEl) {
                    if (stderrEl.textContent === '(no errors)') {
                        stderrEl.textContent = '';
                    }
                    stderrEl.textContent += logContent + '\n';
                }
                
                // Add to logs view
                if (logsContainer) {
                    const logEntry = document.createElement('div');
                    logEntry.className = `log-entry ${data.log_type || 'stdout'}`;
                    const entryContent = (data.content || '').replace(/\n+$/,'');
                    logEntry.innerHTML = `<span style="color: var(--text-muted);">[${formatDate(data.created_at)}]</span> ${escapeHtml(entryContent)}`;
                    logsContainer.appendChild(logEntry);
                    logsContainer.scrollTop = logsContainer.scrollHeight;
                }
            } catch (e) {
                console.error('Failed to handle log event:', e);
            }
        });
        
        eventSource.addEventListener('status', (event) => {
            try {
                const data = JSON.parse(event.data);
                const statusEl = document.getElementById('exec-status');
                if (statusEl) {
                    statusEl.innerHTML = `<span class="${getStatusBadgeClass(data.status)}">${data.status}</span>`;
                }
            } catch (e) {
                console.error('Failed to handle status event:', e);
            }
        });
        
        eventSource.addEventListener('complete', (event) => {
            eventSource.close();
            
            // Hide cancel button, show retry
            document.getElementById('btn-cancel-execution')?.classList.add('hidden');
            document.getElementById('btn-retry-execution')?.classList.remove('hidden');
            
            // Refresh execution data
            this.load(true);
        });
        
        eventSource.onerror = () => {
            eventSource.close();
        };
        
        // Store reference to close when modal closes
        const closeHandler = () => {
            eventSource.close();
            document.getElementById('execution-modal')?.removeEventListener('hidden', closeHandler);
        };
        
        // Close stream when modal is closed
        const observer = new MutationObserver((mutations) => {
            mutations.forEach((mutation) => {
                if (mutation.type === 'attributes' && mutation.attributeName === 'class') {
                    const modal = document.getElementById('execution-modal');
                    if (modal?.classList.contains('hidden')) {
                        eventSource.close();
                        observer.disconnect();
                    }
                }
            });
        });
        
        const modal = document.getElementById('execution-modal');
        if (modal) {
            observer.observe(modal, { attributes: true });
        }
    },
    
    handleStreamEvent(execId, data) {
        // Update table row if visible
        const row = this.tbody?.querySelector(`tr[data-exec-id="${execId}"]`);
        if (row && data.status) {
            const statusCell = row.querySelector('td:nth-child(3)');
            if (statusCell) {
                statusCell.innerHTML = `<span class="${getStatusBadgeClass(data.status)}">${data.status}</span>`;
            }
        }
    },
    
    // Cleanup on page unload
    cleanup() {
        this.activeStreams.forEach((eventSource, execId) => {
            eventSource.close();
        });
        this.activeStreams.clear();
    },
};

// Cleanup on page unload
window.addEventListener('beforeunload', () => {
    ExecutionHistory.cleanup();
});

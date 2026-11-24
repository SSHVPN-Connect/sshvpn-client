# Graph Report - sshvpnproject  (2026-11-24)

## Corpus Check
- cluster-only mode — file stats not available

## Summary
- 696 nodes · 1738 edges · 38 communities (26 shown, 12 thin omitted)
- Extraction: 97% EXTRACTED · 3% INFERRED · 0% AMBIGUOUS · INFERRED: 46 edges (avg confidence: 0.73)
- Token cost: 0 input · 0 output

## Graph Freshness
- Built from commit: `759200c4`
- Run `git rev-parse HEAD` and compare to check if the graph is stale.
- Run `graphify update .` after code changes (no API cost).

## Community Hubs (Navigation)
- ui.rs
- vpn.rs
- Client
- instance.rs
- vpn_service.rs
- ru_ranges.rs
- app_tunnel.rs
- invite_ui.rs
- invites.rs
- tunnel.rs
- Channel
- profiles.rs
- tray.rs
- vpn_log.rs
- AccessEntry
- Profile
- .begin
- Tunnel
- updater.rs
- askpass.rs
- service_main
- redeem
- ServiceRequest
- Picker
- exchange
- open_invitation
- Tray
- AccessChange
- AccessState
- authorized_session
- install-invitations.sh
- install-server.sh
- ssh-tunnel

## God Nodes (most connected - your core abstractions)
1. `Desktop` - 39 edges
2. `Client` - 34 edges
3. `SavedProfile` - 25 edges
4. `tr()` - 23 edges
5. `Channel` - 21 edges
6. `access_row()` - 19 edges
7. `muted()` - 19 edges
8. `Profile` - 18 edges
9. `Tunnel` - 18 edges
10. `InviteUi` - 18 edges

## Surprising Connections (you probably didn't know these)
- `Desktop` --references--> `Listener`  [INFERRED]
  src/ui.rs → src/instance.rs
- `Desktop` --references--> `InviteUi`  [INFERRED]
  src/ui.rs → src/invite_ui.rs
- `Desktop` --references--> `Ipv4Routing`  [INFERRED]
  src/ui.rs → src/profiles.rs
- `Desktop` --references--> `SavedProfile`  [INFERRED]
  src/ui.rs → src/profiles.rs
- `Desktop` --references--> `RuRanges`  [INFERRED]
  src/ui.rs → src/ru_ranges.rs

## Import Cycles
- None detected.

## Communities (38 total, 12 thin omitted)

### Community 0 - "ui.rs"
Cohesion: 0.06
Nodes (72): access_row(), access_rows_keep_compact_height_and_show_status(), accent(), action_icon(), Appearance, Artwork, Assets, background() (+64 more)

### Community 1 - "vpn.rs"
Cohesion: 0.05
Nodes (29): AdapterCache, forward(), HELLO, INVITED_VPN_COMMAND, ipv4_socket(), Log, MTU, MULTI_HELLO (+21 more)

### Community 2 - "Client"
Cohesion: 0.08
Nodes (21): ActiveProfile, Client, Connection, Apps, Socks, Vpn, main(), profile_actions_keep_drafts_and_credentials_separate() (+13 more)

### Community 3 - "instance.rs"
Cohesion: 0.05
Nodes (15): decrypt(), encrypt(), transform(), is_admin(), IsUserAnAdmin(), profile_scope(), run_as_admin(), ShellExecuteW() (+7 more)

### Community 4 - "vpn_service.rs"
Cohesion: 0.06
Nodes (8): app_requests_accept_executable_paths_but_not_arbitrary_commands(), CLIENT_ACCESS, MAX_REQUEST, PIPE, PIPE_BUFFER, POLL, SERVICE, valid_app_paths()

### Community 5 - "ru_ranges.rs"
Cohesion: 0.11
Nodes (17): Ipv4Prefix, split_routes_normalize_prefixes_and_reject_invalid_cidrs(), FALLBACK_SOURCE, generated_routes_cover_exactly_the_complement(), interval(), IPV4_END, load(), load_with() (+9 more)

### Community 6 - "app_tunnel.rs"
Cohesion: 0.11
Nodes (6): AppTunnel, configuration(), configuration_routes_only_selected_executables_to_ssh(), Engine, exact_process_path(), forward_logs()

### Community 7 - "invite_ui.rs"
Cohesion: 0.11
Nodes (12): AccessAction, Ask, Cancel, Confirm, None, InviteUi, last_connection_uses_moscow_time_across_day_boundaries(), moscow_time() (+4 more)

### Community 8 - "invites.rs"
Cohesion: 0.09
Nodes (12): HANDLER, INVITATION_INSTALLER, INVITATIONS_MISSING, INVITATIONS_REQUIRED, Issued, LIST_COMMAND, setup_server(), UPDATE_REQUIRED (+4 more)

### Community 9 - "tunnel.rs"
Cohesion: 0.15
Nodes (3): CREATE_NO_WINDOW, closing_job_terminates_both_tunnel_processes(), TunnelJob

### Community 10 - "Channel"
Cohesion: 0.24
Nodes (6): Channel, is_ipv4_packet(), pump(), serve_apps(), serve_connection(), STOP

### Community 11 - "profiles.rs"
Cohesion: 0.16
Nodes (7): add_unique(), contains(), enumerate(), Process, QUERY, RunningApp, selection_deduplicates_paths_but_keeps_apps_with_the_same_filename()

### Community 13 - "vpn_log.rs"
Cohesion: 0.15
Nodes (3): FILE_LOCK, NEXT_SESSION, SAMPLE_INTERVAL

### Community 14 - "AccessEntry"
Cohesion: 0.18
Nodes (10): Completion, Access, AccessChanged, Imported, Shared, last_successful_access_list_is_cached_per_server(), AccessEntry, Imported (+2 more)

### Community 15 - "Profile"
Cohesion: 0.26
Nodes (7): download(), openssh_can_read_the_pinned_host_file_while_the_profile_is_active(), pin_host(), Profile, ssh_command(), ssh_command_policy(), system_program()

### Community 17 - "Tunnel"
Cohesion: 0.21
Nodes (3): owns_socks_listener(), ssh_error(), Tunnel

### Community 18 - "updater.rs"
Cohesion: 0.27
Nodes (5): ASSET, check(), prepare(), public_updates_ignore_configured_token_and_validate_installer(), update_script()

### Community 19 - "askpass.rs"
Cohesion: 0.22
Nodes (4): configure(), ENDPOINT, run_helper(), TOKEN

### Community 20 - "service_main"
Cohesion: 0.24
Nodes (9): create_listener(), listener(), listener_acl(), named_pipe_transfers_config_and_cleanup_status(), report(), run(), service_main(), service_pid() (+1 more)

### Community 21 - "redeem"
Cohesion: 0.33
Nodes (8): change_access(), generate(), issue(), openssh_accepts_bootstrap_key_in_temp_directory_with_inherited_access(), redeem(), valid_public_key(), valid_user(), write_bootstrap_key()

### Community 22 - "ServiceRequest"
Cohesion: 0.28
Nodes (7): Request, request(), service_rejects_invalid_addresses_routes_and_extra_fields(), service_round_trips_a_profile_and_validates_routes(), ServiceRequest, Apps, System

### Community 24 - "exchange"
Cohesion: 0.38
Nodes (5): configure_server(), exchange(), first_setup_installs_missing_components_and_manual_update_checks_versions(), request(), setup_installer()

### Community 25 - "open_invitation"
Cohesion: 0.43
Nodes (6): invitation_accepts_combined_and_legacy_formats(), needs_legacy_code(), open_invitation(), open_ticket(), seal(), Ticket

### Community 26 - "Tray"
Cohesion: 0.33
Nodes (3): Action, Quit, Tray

### Community 27 - "AccessChange"
Cohesion: 0.40
Nodes (5): AccessChange, RemoveExpired, RemoveRevoked, Restore, Revoke

### Community 28 - "AccessState"
Cohesion: 0.40
Nodes (5): AccessState, Active, Expired, Pending, Revoked

## Knowledge Gaps
- **30 isolated node(s):** `Connection`, `Routing`, `Users`, `Check`, `ConfiguringAdapter` (+25 more)
  These have ≤1 connection - possible missing edges. (Counts symbols only; 216 node(s) total have ≤1 connection when file, concept and rationale nodes are included.)
- **12 thin communities (<3 nodes) omitted from report** — run `graphify query` to explore isolated nodes.

## Suggested Questions
_Questions this graph is uniquely positioned to answer:_

- **Why does `Desktop` connect `ui.rs` to `Client`, `instance.rs`, `ru_ranges.rs`, `invite_ui.rs`, `invites.rs`, `Picker`, `Tray`?**
  _High betweenness centrality (0.094) - this node is a cross-community bridge._
- **Are the 7 inferred relationships involving `Desktop` (e.g. with `Listener` and `InviteUi`) actually correct?**
  _`Desktop` has 7 INFERRED edges - model-reasoned connections that need verification._
- **What connects `Connection`, `Routing`, `Users` to the rest of the system?**
  _30 weakly-connected nodes found - possible missing edges._
- **Should `ui.rs` be split into smaller, more focused modules?**
  _Cohesion score 0.06436695413073366 - nodes in this community are weakly interconnected._
- **Why does `Client` connect `Client` to `ui.rs`, `vpn.rs`, `invite_ui.rs`?**
  _High betweenness centrality (0.081) - this node is a cross-community bridge._
- **Should `vpn.rs` be split into smaller, more focused modules?**
  _Cohesion score 0.05477477477477478 - nodes in this community are weakly interconnected._
- **Why does `SavedProfile` connect `Client` to `ui.rs`, `invites.rs`, `profiles.rs`, `AccessEntry`, `Profile`, `redeem`, `exchange`?**
  _High betweenness centrality (0.043) - this node is a cross-community bridge._
//! LuckPerms' English message templates (`luckperms_en.properties`, MIT, Copyright (c) lucko (Luck) and contributors) and the
//! command usage texts. Generated from the original resource file; `{0}`, `{1}` ... are the arguments.
#![allow(dead_code)]

pub const T_LOGS_ACTIONLOG_PREFIX: &str = "LOG";
pub const T_LOGS_VERBOSE_PREFIX: &str = "VB";
pub const T_LOGS_VERBOSE_HOVER_TYPE_KEY: &str = "Type";
pub const T_LOGS_VERBOSE_HOVER_ORIGIN_KEY: &str = "Origin";
pub const T_LOGS_VERBOSE_HOVER_CAUSE_KEY: &str = "Cause";
pub const T_LOGS_VERBOSE_HOVER_CONTEXT_KEY: &str = "Context";
pub const T_LOGS_VERBOSE_HOVER_THREAD_KEY: &str = "Thread";
pub const T_LOGS_VERBOSE_HOVER_TRACE_KEY: &str = "Trace";
pub const T_LOGS_VERBOSE_HOVER_PROCESSOR_KEY: &str = "Processor";
pub const T_LOGS_VERBOSE_HOVER_OVERFLOW: &str = "and {0} more";
pub const T_LOGS_VERBOSE_RATE_LIMIT_EXCEEDED: &str = "Notification rate limit exceeded. Some events are not being shown. Use {0} to see the full output";
pub const T_LOGS_EXPORT_PREFIX: &str = "EXPORT";
pub const T_COMMANDSYSTEM_AVAILABLE_COMMANDS: &str = "Use {0} to view available commands";
pub const T_COMMANDSYSTEM_COMMAND_NOT_RECOGNISED: &str = "Command not recognised";
pub const T_COMMANDSYSTEM_NO_PERMISSION: &str = "You do not have permission to use this command!";
pub const T_COMMANDSYSTEM_NO_PERMISSION_SUBCOMMANDS: &str = "You do not have permission to use any sub commands";
pub const T_COMMANDSYSTEM_ALREADY_EXECUTING_COMMAND: &str = "Another command is being executed, waiting for it to finish...";
pub const T_COMMANDSYSTEM_COMMANDS_DISABLED: &str = "LuckPerms commands are disabled";
pub const T_COMMANDSYSTEM_USAGE_SUB_COMMANDS_HEADER: &str = "Sub Commands";
pub const T_COMMANDSYSTEM_USAGE_USAGE_HEADER: &str = "Command Usage";
pub const T_COMMANDSYSTEM_USAGE_ARGUMENTS_HEADER: &str = "Arguments";
pub const T_FIRST_TIME_NO_PERMISSIONS_SETUP: &str = "It seems that no permissions have been setup yet!";
pub const T_FIRST_TIME_USE_CONSOLE_TO_GIVE_ACCESS: &str = "Before you can use any of the LuckPerms commands in-game, you need to use the console to give yourself access";
pub const T_FIRST_TIME_CONSOLE_COMMAND_PROMPT: &str = "Open your console and run";
pub const T_FIRST_TIME_NEXT_STEP: &str = "After you've done this, you can begin to define your permission assignments and groups";
pub const T_FIRST_TIME_WIKI_PROMPT: &str = "Don't know where to start? Check here: {0}";
pub const T_FIRST_TIME_CLICK_TO_COPY: &str = "Click to copy";
pub const T_LOGIN_TRY_AGAIN: &str = "Please try again later";
pub const T_LOGIN_LOADING_DATABASE_ERROR: &str = "A database error occurred whilst loading permissions data";
pub const T_LOGIN_SERVER_ADMIN_CHECK_CONSOLE_ERRORS: &str = "If you are a server admin, please check the console for any errors";
pub const T_LOGIN_SERVER_ADMIN_CHECK_CONSOLE_INFO: &str = "Please check the server console for more information";
pub const T_LOGIN_DATA_NOT_LOADED_AT_PRE: &str = "Permissions data for your user was not loaded during the pre-login stage";
pub const T_LOGIN_UNABLE_TO_CONTINUE: &str = "unable to continue";
pub const T_LOGIN_CRAFTBUKKIT_OFFLINE_MODE_ERROR: &str = "this is likely due to a conflict between CraftBukkit and the online-mode setting";
pub const T_LOGIN_UNEXPECTED_ERROR: &str = "An unexpected error occurred whilst setting up your permissions data";
pub const T_OPSYSTEM_DISABLED: &str = "The vanilla OP system is disabled on this server";
pub const T_OPSYSTEM_SPONGE_WARNING: &str = "Please note that Server Operator status has no effect on Sponge permission checks when a permission plugin is installed, you must edit user data directly";
pub const T_DURATION_UNIT_YEARS_PLURAL: &str = "{0} years";
pub const T_DURATION_UNIT_YEARS_SINGULAR: &str = "{0} year";
pub const T_DURATION_UNIT_YEARS_SHORT: &str = "{0}y";
pub const T_DURATION_UNIT_MONTHS_PLURAL: &str = "{0} months";
pub const T_DURATION_UNIT_MONTHS_SINGULAR: &str = "{0} month";
pub const T_DURATION_UNIT_MONTHS_SHORT: &str = "{0}mo";
pub const T_DURATION_UNIT_WEEKS_PLURAL: &str = "{0} weeks";
pub const T_DURATION_UNIT_WEEKS_SINGULAR: &str = "{0} week";
pub const T_DURATION_UNIT_WEEKS_SHORT: &str = "{0}w";
pub const T_DURATION_UNIT_DAYS_PLURAL: &str = "{0} days";
pub const T_DURATION_UNIT_DAYS_SINGULAR: &str = "{0} day";
pub const T_DURATION_UNIT_DAYS_SHORT: &str = "{0}d";
pub const T_DURATION_UNIT_HOURS_PLURAL: &str = "{0} hours";
pub const T_DURATION_UNIT_HOURS_SINGULAR: &str = "{0} hour";
pub const T_DURATION_UNIT_HOURS_SHORT: &str = "{0}h";
pub const T_DURATION_UNIT_MINUTES_PLURAL: &str = "{0} minutes";
pub const T_DURATION_UNIT_MINUTES_SINGULAR: &str = "{0} minute";
pub const T_DURATION_UNIT_MINUTES_SHORT: &str = "{0}m";
pub const T_DURATION_UNIT_SECONDS_PLURAL: &str = "{0} seconds";
pub const T_DURATION_UNIT_SECONDS_SINGULAR: &str = "{0} second";
pub const T_DURATION_UNIT_SECONDS_SHORT: &str = "{0}s";
pub const T_DURATION_SINCE: &str = "{0} ago";
pub const T_DURATION_DATE: &str = "Date";
pub const T_COMMAND_MISC_INVALID_CODE: &str = "Invalid code";
pub const T_COMMAND_MISC_RESPONSE_CODE_KEY: &str = "response code";
pub const T_COMMAND_MISC_ERROR_MESSAGE_KEY: &str = "message";
pub const T_COMMAND_MISC_BYTEBIN_UNABLE_TO_COMMUNICATE: &str = "Unable to communicate with bytebin";
pub const T_COMMAND_MISC_WEBAPP_UNABLE_TO_COMMUNICATE: &str = "Unable to communicate with the web app";
pub const T_COMMAND_MISC_CHECK_CONSOLE_FOR_ERRORS: &str = "Check the console for errors";
pub const T_COMMAND_MISC_FILE_MUST_BE_IN_DATA: &str = "File {0} must be a direct child of the data directory";
pub const T_COMMAND_MISC_WAIT_TO_FINISH: &str = "Please wait for it to finish and try again";
pub const T_COMMAND_MISC_INVALID_INPUT_EMPTY_PERMISSION: &str = "The empty string is not a valid permission";
pub const T_COMMAND_MISC_INVALID_INPUT_EMPTY_META_KEY: &str = "The empty string is not a valid meta key";
pub const T_COMMAND_MISC_INVALID_INPUT_EMPTY_DISPLAY_NAME: &str = "The empty string is not a valid display name";
pub const T_COMMAND_MISC_SHORTHAND_PARSE_ERROR: &str = "Warning: Permission {0} could not be parsed as shorthand: {1}";
pub const T_COMMAND_MISC_INVALID_PRIORITY: &str = "Invalid priority {0}";
pub const T_COMMAND_MISC_EXPECTED_NUMBER: &str = "Expected a number";
pub const T_COMMAND_MISC_DATE_PARSE_ERROR: &str = "Could not parse date {0}";
pub const T_COMMAND_MISC_DATE_IN_PAST_ERROR: &str = "You cannot set a date in the past!";
pub const T_COMMAND_MISC_PAGE: &str = "page {0} of {1}";
pub const T_COMMAND_MISC_PAGE_ENTRIES: &str = "{0} entries";
pub const T_COMMAND_MISC_NONE: &str = "None";
pub const T_COMMAND_MISC_LOADING_ERROR_UNEXPECTED: &str = "An unexpected error occurred";
pub const T_COMMAND_MISC_LOADING_ERROR_USER: &str = "User not loaded";
pub const T_COMMAND_MISC_LOADING_ERROR_USER_SPECIFIC: &str = "Unable to load target user {0}";
pub const T_COMMAND_MISC_LOADING_ERROR_USER_NOT_FOUND: &str = "A user for {0} could not be found";
pub const T_COMMAND_MISC_LOADING_ERROR_USER_SAVE_ERROR: &str = "There was an error whilst saving user data for {0}";
pub const T_COMMAND_MISC_LOADING_ERROR_USER_NOT_ONLINE: &str = "User {0} is not online";
pub const T_COMMAND_MISC_LOADING_ERROR_USER_INVALID: &str = "{0} is not a valid username/uuid";
pub const T_COMMAND_MISC_LOADING_ERROR_USER_NOT_UUID: &str = "Target user {0} is not a valid uuid";
pub const T_COMMAND_MISC_LOADING_ERROR_GROUP: &str = "Group not loaded";
pub const T_COMMAND_MISC_LOADING_ERROR_ALL_GROUPS: &str = "Unable to load all groups";
pub const T_COMMAND_MISC_LOADING_ERROR_GROUP_NOT_FOUND: &str = "A group named {0} could not be found";
pub const T_COMMAND_MISC_LOADING_ERROR_GROUP_SAVE_ERROR: &str = "There was an error whilst saving group data for {0}";
pub const T_COMMAND_MISC_LOADING_ERROR_GROUP_INVALID: &str = "{0} is not a valid group name";
pub const T_COMMAND_MISC_LOADING_ERROR_TRACK: &str = "Track not loaded";
pub const T_COMMAND_MISC_LOADING_ERROR_ALL_TRACKS: &str = "Unable to load all tracks";
pub const T_COMMAND_MISC_LOADING_ERROR_TRACK_NOT_FOUND: &str = "A track named {0} could not be found";
pub const T_COMMAND_MISC_LOADING_ERROR_TRACK_SAVE_ERROR: &str = "There was an error whilst saving track data for {0}";
pub const T_COMMAND_MISC_LOADING_ERROR_TRACK_INVALID: &str = "{0} is not a valid track name";
pub const T_COMMAND_MISC_NODE_PERMISSION: &str = "Permission";
pub const T_COMMAND_MISC_NODE_INHERITANCE: &str = "Inheritance";
pub const T_COMMAND_MISC_NODE_PREFIX: &str = "Prefix";
pub const T_COMMAND_MISC_NODE_SUFFIX: &str = "Suffix";
pub const T_COMMAND_MISC_NODE_META: &str = "Meta";
pub const T_COMMAND_MISC_NODE_WEIGHT: &str = "Weight";
pub const T_COMMAND_MISC_NODE_DISPLAYNAME: &str = "Display Name";
pub const T_COMMAND_MISC_PRIORITY_LABEL: &str = "priority: {0}";
pub const T_COMMAND_EDITOR_NO_MATCH: &str = "Unable to open editor, no objects matched the desired type";
pub const T_COMMAND_EDITOR_START: &str = "Preparing a new editor session, please wait...";
pub const T_COMMAND_EDITOR_URL: &str = "Click the link below to open the editor";
pub const T_COMMAND_EDITOR_SOCKET_CONNECTED: &str = "Editor window connected successfully";
pub const T_COMMAND_EDITOR_SOCKET_RECONNECTED: &str = "Editor window reconnected successfully";
pub const T_COMMAND_EDITOR_SOCKET_CHANGES_RECEIVED: &str = "Changes have been received from the connected web editor session";
pub const T_COMMAND_EDITOR_SOCKET_UNTRUSTED: &str = "An editor window has connected, but it is not yet trusted";
pub const T_COMMAND_EDITOR_SOCKET_UNTRUSTED_PROMPT_CLICK: &str = "If it was you, {0} to trust the session!";
pub const T_COMMAND_EDITOR_SOCKET_UNTRUSTED_PROMPT_CLICK_ACTION: &str = "click here";
pub const T_COMMAND_EDITOR_SOCKET_UNTRUSTED_PROMPT_RUNCOMMAND: &str = "If it was you, run {0} to trust the session!";
pub const T_COMMAND_EDITOR_SOCKET_UNTRUSTED_SESSIONINFO: &str = "session id = {0}, browser = {1}";
pub const T_COMMAND_EDITOR_SOCKET_TRUST_SUCCESS: &str = "The editor session has been marked as trusted";
pub const T_COMMAND_EDITOR_SOCKET_TRUST_FUTUREINFO: &str = "In the future, connections from the same browser will be trusted automatically";
pub const T_COMMAND_EDITOR_SOCKET_TRUST_CONNECTING: &str = "The plugin will now attempt to establish a connection with the editor...";
pub const T_COMMAND_EDITOR_SOCKET_TRUST_FAILURE: &str = "Unable to trust the given session because the socket is closed, or because a different connection was established instead";
pub const T_COMMAND_EDITOR_UNABLE_TO_COMMUNICATE: &str = "Unable to communicate with the editor";
pub const T_COMMAND_EDITOR_APPLY_EDITS_SUCCESS: &str = "Web editor data was applied to {0} {1} successfully";
pub const T_COMMAND_EDITOR_APPLY_EDITS_SUCCESS_SUMMARY: &str = "{0} {1} and {2} {3}";
pub const T_COMMAND_EDITOR_APPLY_EDITS_SUCCESS_ADDITIONS: &str = "additions";
pub const T_COMMAND_EDITOR_APPLY_EDITS_SUCCESS_ADDITIONS_SINGULAR: &str = "addition";
pub const T_COMMAND_EDITOR_APPLY_EDITS_SUCCESS_DELETIONS: &str = "deletions";
pub const T_COMMAND_EDITOR_APPLY_EDITS_SUCCESS_DELETIONS_SINGULAR: &str = "deletion";
pub const T_COMMAND_EDITOR_APPLY_EDITS_NO_CHANGES: &str = "No changes were applied from the web editor, the returned data didn't contain any edits";
pub const T_COMMAND_EDITOR_APPLY_EDITS_UNKNOWN_TYPE: &str = "Unable to apply edit to the specified object type";
pub const T_COMMAND_EDITOR_APPLY_EDITS_UNABLE_TO_READ: &str = "Unable to read data using the given code";
pub const T_COMMAND_EDITOR_APPLY_EDITS_UNKNOWN_SESSION: &str = "The changes received from the web editor were not made in a session started on this server!";
pub const T_COMMAND_EDITOR_APPLY_EDITS_RIGHT_SERVER_QUESTION: &str = "Are you sure you're running the {0} command in the right place?";
pub const T_COMMAND_EDITOR_APPLY_EDITS_ALREADY_APPLIED: &str = "The changes received from the web editor are based on an initial session which has already been applied!";
pub const T_COMMAND_EDITOR_APPLY_EDITS_HOW_TO_AVOID_CONFLICTS: &str = "To avoid conflicts, you should never re-use the same editor session after the changes from it have been applied once already";
pub const T_COMMAND_EDITOR_APPLY_EDITS_BYPASS_WARNING: &str = "To ignore this warning and apply the changes anyway, run";
pub const T_COMMAND_SEARCH_SEARCHING_PERMISSION: &str = "Searching for users and groups with {0}";
pub const T_COMMAND_SEARCH_SEARCHING_INHERIT: &str = "Searching for users and groups who inherit from {0}";
pub const T_COMMAND_SEARCH_RESULT: &str = "Found {0} entries from {1} users and {2} groups";
pub const T_COMMAND_SEARCH_RESULT_DEFAULT_NOTICE: &str = "Note: when searching for members of the default group, offline players with no other permissions will not be shown!";
pub const T_COMMAND_SEARCH_SHOWING_USERS: &str = "Showing user entries";
pub const T_COMMAND_SEARCH_SHOWING_GROUPS: &str = "Showing group entries";
pub const T_COMMAND_TREE_START: &str = "Generating permission tree, please wait...";
pub const T_COMMAND_TREE_EMPTY: &str = "Unable to generate tree, no results were found";
pub const T_COMMAND_TREE_URL: &str = "Permission tree URL";
pub const T_COMMAND_VERBOSE_INVALID_FILTER: &str = "{0} is not a valid verbose filter";
pub const T_COMMAND_VERBOSE_ENABLED: &str = "Verbose logging {0} for checks matching {1}";
pub const T_COMMAND_VERBOSE_COMMAND_EXEC: &str = "Forcing {0} to execute command {1} and reporting all checks made...";
pub const T_COMMAND_VERBOSE_OFF: &str = "Verbose logging {0}";
pub const T_COMMAND_VERBOSE_COMMAND_EXEC_COMPLETE: &str = "Command execution complete";
pub const T_COMMAND_VERBOSE_COMMAND_NO_CHECKS: &str = "The command execution completed, but no permission checks were made";
pub const T_COMMAND_VERBOSE_COMMAND_POSSIBLY_ASYNC: &str = "This might be because the plugin runs commands in the background (async)";
pub const T_COMMAND_VERBOSE_COMMAND_TRY_AGAIN_MANUALLY: &str = "You can still use verbose manually to detect checks made like this";
pub const T_COMMAND_VERBOSE_ENABLED_RECORDING: &str = "Verbose recording {0} for checks matching {1}";
pub const T_COMMAND_VERBOSE_UPLOADING: &str = "Verbose logging {0}, uploading results...";
pub const T_COMMAND_VERBOSE_URL: &str = "Verbose results URL";
pub const T_COMMAND_VERBOSE_ENABLED_TERM: &str = "enabled";
pub const T_COMMAND_VERBOSE_DISABLED_TERM: &str = "disabled";
pub const T_COMMAND_VERBOSE_QUERY_ANY: &str = "ANY";
pub const T_COMMAND_INFO_RUNNING_PLUGIN: &str = "Running";
pub const T_COMMAND_INFO_PLATFORM_KEY: &str = "Platform";
pub const T_COMMAND_INFO_SERVER_BRAND_KEY: &str = "Server Brand";
pub const T_COMMAND_INFO_SERVER_VERSION_KEY: &str = "Server Version";
pub const T_COMMAND_INFO_STORAGE_KEY: &str = "Storage";
pub const T_COMMAND_INFO_STORAGE_TYPE_KEY: &str = "Type";
pub const T_COMMAND_INFO_STORAGE_META_PING_KEY: &str = "Ping";
pub const T_COMMAND_INFO_STORAGE_META_CONNECTED_KEY: &str = "Connected";
pub const T_COMMAND_INFO_STORAGE_META_FILE_SIZE_KEY: &str = "File Size";
pub const T_COMMAND_INFO_EXTENSIONS_KEY: &str = "Extensions";
pub const T_COMMAND_INFO_MESSAGING_KEY: &str = "Messaging";
pub const T_COMMAND_INFO_INSTANCE_KEY: &str = "Instance";
pub const T_COMMAND_INFO_STATIC_CONTEXTS_KEY: &str = "Static contexts";
pub const T_COMMAND_INFO_ONLINE_PLAYERS_KEY: &str = "Online Players";
pub const T_COMMAND_INFO_ONLINE_PLAYERS_UNIQUE: &str = "{0} unique";
pub const T_COMMAND_INFO_UPTIME_KEY: &str = "Uptime";
pub const T_COMMAND_INFO_LOCAL_DATA_KEY: &str = "Local Data";
pub const T_COMMAND_INFO_LOCAL_DATA: &str = "{0} users, {1} groups, {2} tracks";
pub const T_COMMAND_GENERIC_CREATE_SUCCESS: &str = "{0} was successfully created";
pub const T_COMMAND_GENERIC_CREATE_ERROR: &str = "There was an error whilst creating {0}";
pub const T_COMMAND_GENERIC_CREATE_ERROR_ALREADY_EXISTS: &str = "{0} already exists!";
pub const T_COMMAND_GENERIC_DELETE_SUCCESS: &str = "{0} was successfully deleted";
pub const T_COMMAND_GENERIC_DELETE_ERROR: &str = "There was an error whilst deleting {0}";
pub const T_COMMAND_GENERIC_DELETE_ERROR_DOESNT_EXIST: &str = "{0} does not exist!";
pub const T_COMMAND_GENERIC_RENAME_SUCCESS: &str = "{0} was successfully renamed to {1}";
pub const T_COMMAND_GENERIC_CLONE_SUCCESS: &str = "{0} was successfully cloned onto {1}";
pub const T_COMMAND_GENERIC_INFO_PARENT_TITLE: &str = "Parent Groups";
pub const T_COMMAND_GENERIC_INFO_PARENT_TEMPORARY_TITLE: &str = "Temporary Parent Groups";
pub const T_COMMAND_GENERIC_INFO_EXPIRES_IN: &str = "expires in";
pub const T_COMMAND_GENERIC_INFO_INHERITED_FROM: &str = "inherited from";
pub const T_COMMAND_GENERIC_INFO_INHERITED_FROM_SELF: &str = "self";
pub const T_COMMAND_GENERIC_SHOW_TRACKS_TITLE: &str = "{0}'s Tracks";
pub const T_COMMAND_GENERIC_SHOW_TRACKS_EMPTY: &str = "{0} is not on any tracks";
pub const T_COMMAND_GENERIC_CLEAR_NODE_REMOVED: &str = "{0} nodes were removed";
pub const T_COMMAND_GENERIC_CLEAR_NODE_REMOVED_SINGULAR: &str = "{0} node was removed";
pub const T_COMMAND_GENERIC_CLEAR: &str = "{0}'s nodes were cleared in context {1}";
pub const T_COMMAND_GENERIC_PERMISSION_INFO_TITLE: &str = "{0}'s Permissions";
pub const T_COMMAND_GENERIC_PERMISSION_INFO_EMPTY: &str = "{0} does not have any permissions set";
pub const T_COMMAND_GENERIC_PERMISSION_INFO_CLICK_TO_REMOVE: &str = "Click to remove this node from {0}";
pub const T_COMMAND_GENERIC_PERMISSION_CHECK_INFO_TITLE: &str = "Permission information for {0}";
pub const T_COMMAND_GENERIC_PERMISSION_CHECK_INFO_DIRECTLY: &str = "{0} has {1} set to {2} in context {3}";
pub const T_COMMAND_GENERIC_PERMISSION_CHECK_INFO_INHERITED: &str = "{0} inherits {1} set to {2} from {3} in context {4}";
pub const T_COMMAND_GENERIC_PERMISSION_CHECK_INFO_NOT_DIRECTLY: &str = "{0} does not have {1} set";
pub const T_COMMAND_GENERIC_PERMISSION_CHECK_INFO_NOT_INHERITED: &str = "{0} does not inherit {1}";
pub const T_COMMAND_GENERIC_PERMISSION_CHECK_RESULT_TITLE: &str = "Permission check for {0}";
pub const T_COMMAND_GENERIC_PERMISSION_CHECK_RESULT_RESULT_KEY: &str = "Result";
pub const T_COMMAND_GENERIC_PERMISSION_CHECK_RESULT_PROCESSOR_KEY: &str = "Processor";
pub const T_COMMAND_GENERIC_PERMISSION_CHECK_RESULT_CAUSE_KEY: &str = "Cause";
pub const T_COMMAND_GENERIC_PERMISSION_CHECK_RESULT_CONTEXT_KEY: &str = "Context";
pub const T_COMMAND_GENERIC_PERMISSION_SET: &str = "Set {0} to {1} for {2} in context {3}";
pub const T_COMMAND_GENERIC_PERMISSION_ALREADY_HAS: &str = "{0} already has {1} set in context {2}";
pub const T_COMMAND_GENERIC_PERMISSION_SET_TEMP: &str = "Set {0} to {1} for {2} for a duration of {3} in context {4}";
pub const T_COMMAND_GENERIC_PERMISSION_ALREADY_HAS_TEMP: &str = "{0} already has {1} set temporarily in context {2}";
pub const T_COMMAND_GENERIC_PERMISSION_UNSET: &str = "Unset {0} for {1} in context {2}";
pub const T_COMMAND_GENERIC_PERMISSION_DOESNT_HAVE: &str = "{0} does not have {1} set in context {2}";
pub const T_COMMAND_GENERIC_PERMISSION_UNSET_TEMP: &str = "Unset temporary permission {0} for {1} in context {2}";
pub const T_COMMAND_GENERIC_PERMISSION_SUBTRACT: &str = "Set {0} to {1} for {2} for a duration of {3} in context {4}, {5} less than before";
pub const T_COMMAND_GENERIC_PERMISSION_DOESNT_HAVE_TEMP: &str = "{0} does not have {1} set temporarily in context {2}";
pub const T_COMMAND_GENERIC_PERMISSION_CLEAR: &str = "{0}'s permissions were cleared in context {1}";
pub const T_COMMAND_GENERIC_PARENT_INFO_TITLE: &str = "{0}'s Parents";
pub const T_COMMAND_GENERIC_PARENT_INFO_EMPTY: &str = "{0} does not have any parents defined";
pub const T_COMMAND_GENERIC_PARENT_INFO_CLICK_TO_REMOVE: &str = "Click to remove this parent from {0}";
pub const T_COMMAND_GENERIC_PARENT_ADD: &str = "{0} now inherits permissions from {1} in context {2}";
pub const T_COMMAND_GENERIC_PARENT_ADD_TEMP: &str = "{0} now inherits permissions from {1} for a duration of {2} in context {3}";
pub const T_COMMAND_GENERIC_PARENT_SET: &str = "{0} had their existing parent groups cleared, and now only inherits {1} in context {2}";
pub const T_COMMAND_GENERIC_PARENT_SET_TRACK: &str = "{0} had their existing parent groups on track {1} cleared, and now only inherits {2} in context {3}";
pub const T_COMMAND_GENERIC_PARENT_REMOVE: &str = "{0} no longer inherits permissions from {1} in context {2}";
pub const T_COMMAND_GENERIC_PARENT_REMOVE_TEMP: &str = "{0} no longer temporarily inherits permissions from {1} in context {2}";
pub const T_COMMAND_GENERIC_PARENT_SUBTRACT: &str = "{0} will inherit permissions from {1} for a duration of {2} in context {3}, {4} less than before";
pub const T_COMMAND_GENERIC_PARENT_CLEAR: &str = "{0}'s parents were cleared in context {1}";
pub const T_COMMAND_GENERIC_PARENT_CLEAR_TRACK: &str = "{0}'s parents on track {1} were cleared in context {2}";
pub const T_COMMAND_GENERIC_PARENT_ALREADY_INHERITS: &str = "{0} already inherits from {1} in context {2}";
pub const T_COMMAND_GENERIC_PARENT_DOESNT_INHERIT: &str = "{0} does not inherit from {1} in context {2}";
pub const T_COMMAND_GENERIC_PARENT_ALREADY_TEMP_INHERITS: &str = "{0} already temporarily inherits from {1} in context {2}";
pub const T_COMMAND_GENERIC_PARENT_DOESNT_TEMP_INHERIT: &str = "{0} does not temporarily inherit from {1} in context {2}";
pub const T_COMMAND_GENERIC_CHAT_META_INFO_TITLE_PREFIX: &str = "{0}'s Prefixes";
pub const T_COMMAND_GENERIC_CHAT_META_INFO_TITLE_SUFFIX: &str = "{0}'s Suffixes";
pub const T_COMMAND_GENERIC_CHAT_META_INFO_NONE_PREFIX: &str = "{0} has no prefixes";
pub const T_COMMAND_GENERIC_CHAT_META_INFO_NONE_SUFFIX: &str = "{0} has no suffixes";
pub const T_COMMAND_GENERIC_CHAT_META_INFO_CLICK_TO_REMOVE: &str = "Click to remove this {0} from {1}";
pub const T_COMMAND_GENERIC_CHAT_META_ALREADY_HAS: &str = "{0} already has {1} {2} set at a priority of {3} in context {4}";
pub const T_COMMAND_GENERIC_CHAT_META_ALREADY_HAS_TEMP: &str = "{0} already has {1} {2} set temporarily at a priority of {3} in context {4}";
pub const T_COMMAND_GENERIC_CHAT_META_DOESNT_HAVE: &str = "{0} doesn't have {1} {2} set at a priority of {3} in context {4}";
pub const T_COMMAND_GENERIC_CHAT_META_DOESNT_HAVE_TEMP: &str = "{0} doesn't have {1} {2} set temporarily at a priority of {3} in context {4}";
pub const T_COMMAND_GENERIC_CHAT_META_ADD: &str = "{0} had {1} {2} set at a priority of {3} in context {4}";
pub const T_COMMAND_GENERIC_CHAT_META_ADD_TEMP: &str = "{0} had {1} {2} set at a priority of {3} for a duration of {4} in context {5}";
pub const T_COMMAND_GENERIC_CHAT_META_REMOVE: &str = "{0} had {1} {2} at priority {3} removed in context {4}";
pub const T_COMMAND_GENERIC_CHAT_META_REMOVE_BULK: &str = "{0} had all {1} at priority {2} removed in context {3}";
pub const T_COMMAND_GENERIC_CHAT_META_REMOVE_TEMP: &str = "{0} had temporary {1} {2} at priority {3} removed in context {4}";
pub const T_COMMAND_GENERIC_CHAT_META_REMOVE_TEMP_BULK: &str = "{0} had all temporary {1} at priority {2} removed in context {3}";
pub const T_COMMAND_GENERIC_META_INFO_TITLE: &str = "{0}'s Meta";
pub const T_COMMAND_GENERIC_META_INFO_NONE: &str = "{0} has no meta";
pub const T_COMMAND_GENERIC_META_INFO_CLICK_TO_REMOVE: &str = "Click to remove this meta node from {0}";
pub const T_COMMAND_GENERIC_META_ALREADY_HAS: &str = "{0} already has meta key {1} set to {2} in context {3}";
pub const T_COMMAND_GENERIC_META_ALREADY_HAS_TEMP: &str = "{0} already has meta key {1} temporarily set to {2} in context {3}";
pub const T_COMMAND_GENERIC_META_DOESNT_HAVE: &str = "{0} doesn't have meta key {1} set in context {2}";
pub const T_COMMAND_GENERIC_META_DOESNT_HAVE_TEMP: &str = "{0} doesn't have meta key {1} set temporarily in context {2}";
pub const T_COMMAND_GENERIC_META_SET: &str = "Set meta key {0} to {1} for {2} in context {3}";
pub const T_COMMAND_GENERIC_META_SET_TEMP: &str = "Set meta key {0} to {1} for {2} for a duration of {3} in context {4}";
pub const T_COMMAND_GENERIC_META_UNSET: &str = "Unset meta key {0} for {1} in context {2}";
pub const T_COMMAND_GENERIC_META_UNSET_TEMP: &str = "Unset temporary meta key {0} for {1} in context {2}";
pub const T_COMMAND_GENERIC_META_CLEAR: &str = "{0}'s meta matching type {1} was cleared in context {2}";
pub const T_COMMAND_GENERIC_CONTEXTUAL_DATA_TITLE: &str = "Contextual Data";
pub const T_COMMAND_GENERIC_CONTEXTUAL_DATA_MODE_KEY: &str = "mode";
pub const T_COMMAND_GENERIC_CONTEXTUAL_DATA_MODE_SERVER: &str = "server";
pub const T_COMMAND_GENERIC_CONTEXTUAL_DATA_MODE_ACTIVE_PLAYER: &str = "active player";
pub const T_COMMAND_GENERIC_CONTEXTUAL_DATA_CONTEXTS_KEY: &str = "Contexts";
pub const T_COMMAND_GENERIC_CONTEXTUAL_DATA_PREFIX_KEY: &str = "Prefix";
pub const T_COMMAND_GENERIC_CONTEXTUAL_DATA_SUFFIX_KEY: &str = "Suffix";
pub const T_COMMAND_GENERIC_CONTEXTUAL_DATA_PRIMARY_GROUP_KEY: &str = "Primary Group";
pub const T_COMMAND_GENERIC_CONTEXTUAL_DATA_META_KEY: &str = "Meta";
pub const T_COMMAND_GENERIC_CONTEXTUAL_DATA_NULL_RESULT: &str = "None";
pub const T_COMMAND_USER_INFO_TITLE: &str = "User Info";
pub const T_COMMAND_USER_INFO_UUID_KEY: &str = "UUID";
pub const T_COMMAND_USER_INFO_UUID_TYPE_KEY: &str = "type";
pub const T_COMMAND_USER_INFO_UUID_TYPE_MOJANG: &str = "official";
pub const T_COMMAND_USER_INFO_UUID_TYPE_NOT_MOJANG: &str = "offline";
pub const T_COMMAND_USER_INFO_UUID_TYPE_NPC: &str = "npc";
pub const T_COMMAND_USER_INFO_UUID_TYPE_UNKNOWN: &str = "unknown";
pub const T_COMMAND_USER_INFO_UUID_TYPE_DESC_MOJANG: &str = "This is a unique id for an official Minecraft: Java Edition account (online mode)";
pub const T_COMMAND_USER_INFO_UUID_TYPE_DESC_NOT_MOJANG: &str = "This is a pseudo unique id generated from the username (offline mode)";
pub const T_COMMAND_USER_INFO_UUID_TYPE_DESC_NPC: &str = "This is a unique id for a NPC (non-player character)";
pub const T_COMMAND_USER_INFO_UUID_TYPE_DESC_UNKNOWN: &str = "This is an unknown unique id type";
pub const T_COMMAND_USER_INFO_UUID_TYPE_DESC_API: &str = "This is a unique id type specified via the LuckPerms API";
pub const T_COMMAND_USER_INFO_STATUS_KEY: &str = "Status";
pub const T_COMMAND_USER_INFO_STATUS_ONLINE: &str = "Online";
pub const T_COMMAND_USER_INFO_STATUS_OFFLINE: &str = "Offline";
pub const T_COMMAND_USER_REMOVEGROUP_ERROR_PRIMARY: &str = "You cannot remove a user from their primary group";
pub const T_COMMAND_USER_PRIMARYGROUP_NOT_MEMBER: &str = "{0} was not already a member of {1}, adding them now";
pub const T_COMMAND_USER_PRIMARYGROUP_ALREADY_HAS: &str = "{0} already has {1} set as their primary group";
pub const T_COMMAND_USER_PRIMARYGROUP_WARN_OPTION: &str = "Warning: The primary group calculation method being used by this server ({0}) may not reflect this change";
pub const T_COMMAND_USER_PRIMARYGROUP_SET: &str = "{0}'s primary group was set to {1}";
pub const T_COMMAND_USER_TRACK_ERROR_NOT_CONTAIN_GROUP: &str = "{0} isn't already in any groups on {1}";
pub const T_COMMAND_USER_TRACK_UNSURE_WHICH_TRACK: &str = "Unsure which track to use, please specify it as an argument";
pub const T_COMMAND_USER_TRACK_MISSING_GROUP_ADVICE: &str = "Either create the group, or remove it from the track and try again";
pub const T_COMMAND_USER_PROMOTE_ADDED_TO_FIRST: &str = "{0} isn't in any groups on {1}, so they were added to the first group, {2} in context {3}";
pub const T_COMMAND_USER_PROMOTE_NOT_ON_TRACK: &str = "{0} isn't in any groups on {1}, so was not promoted";
pub const T_COMMAND_USER_PROMOTE_SUCCESS: &str = "Promoting {0} along track {1} from {2} to {3} in context {4}";
pub const T_COMMAND_USER_PROMOTE_END_OF_TRACK: &str = "The end of track {0} was reached, unable to promote {1}";
pub const T_COMMAND_USER_PROMOTE_NEXT_GROUP_DELETED: &str = "The next group on the track, {0}, no longer exists";
pub const T_COMMAND_USER_PROMOTE_UNABLE_TO_PROMOTE: &str = "Unable to promote user";
pub const T_COMMAND_USER_DEMOTE_SUCCESS: &str = "Demoting {0} along track {1} from {2} to {3} in context {4}";
pub const T_COMMAND_USER_DEMOTE_END_OF_TRACK: &str = "The end of track {0} was reached, so {1} was removed from {2}";
pub const T_COMMAND_USER_DEMOTE_END_OF_TRACK_NOT_REMOVED: &str = "The end of track {0} was reached, but {1} was not removed from the first group";
pub const T_COMMAND_USER_DEMOTE_PREVIOUS_GROUP_DELETED: &str = "The previous group on the track, {0}, no longer exists";
pub const T_COMMAND_USER_DEMOTE_UNABLE_TO_DEMOTE: &str = "Unable to demote user";
pub const T_COMMAND_GROUP_LIST_TITLE: &str = "Groups";
pub const T_COMMAND_GROUP_DELETE_NOT_DEFAULT: &str = "You cannot delete the default group";
pub const T_COMMAND_GROUP_INFO_TITLE: &str = "Group Info";
pub const T_COMMAND_GROUP_INFO_DISPLAY_NAME_KEY: &str = "Display Name";
pub const T_COMMAND_GROUP_INFO_WEIGHT_KEY: &str = "Weight";
pub const T_COMMAND_GROUP_SETWEIGHT_SET: &str = "Set weight to {0} for group {1}";
pub const T_COMMAND_GROUP_SETDISPLAYNAME_DOESNT_HAVE: &str = "{0} doesn't have a display name set";
pub const T_COMMAND_GROUP_SETDISPLAYNAME_ALREADY_HAS: &str = "{0} already has a display name of {1}";
pub const T_COMMAND_GROUP_SETDISPLAYNAME_ALREADY_IN_USE: &str = "The display name {0} is already being used by {1}";
pub const T_COMMAND_GROUP_SETDISPLAYNAME_SET: &str = "Set display name to {0} for group {1} in context {2}";
pub const T_COMMAND_GROUP_SETDISPLAYNAME_REMOVED: &str = "Removed display name for group {0} in context {1}";
pub const T_COMMAND_TRACK_LIST_TITLE: &str = "Tracks";
pub const T_COMMAND_TRACK_PATH_EMPTY: &str = "None";
pub const T_COMMAND_TRACK_INFO_SHOWING_TRACK: &str = "Showing Track";
pub const T_COMMAND_TRACK_INFO_PATH_PROPERTY: &str = "Path";
pub const T_COMMAND_TRACK_CLEAR: &str = "{0}'s groups track was cleared";
pub const T_COMMAND_TRACK_APPEND_SUCCESS: &str = "Group {0} was appended to track {1}";
pub const T_COMMAND_TRACK_INSERT_SUCCESS: &str = "Group {0} was inserted into track {1} at position {2}";
pub const T_COMMAND_TRACK_INSERT_ERROR_NUMBER: &str = "Expected number but instead received: {0}";
pub const T_COMMAND_TRACK_INSERT_ERROR_INVALID_POS: &str = "Unable to insert at position {0}";
pub const T_COMMAND_TRACK_INSERT_ERROR_INVALID_POS_REASON: &str = "invalid position";
pub const T_COMMAND_TRACK_REMOVE_SUCCESS: &str = "Group {0} was removed from track {1}";
pub const T_COMMAND_TRACK_ERROR_EMPTY: &str = "{0} cannot be used as it is empty or contains only one group";
pub const T_COMMAND_TRACK_ERROR_MULTIPLE_GROUPS: &str = "{0} is a member of multiple groups on this track";
pub const T_COMMAND_TRACK_ERROR_AMBIGUOUS: &str = "Unable to determine their location";
pub const T_COMMAND_TRACK_ALREADY_CONTAINS: &str = "{0} already contains {1}";
pub const T_COMMAND_TRACK_DOESNT_CONTAIN: &str = "{0} doesn't contain {1}";
pub const T_COMMAND_LOG_LOAD_ERROR: &str = "The log could not be loaded";
pub const T_COMMAND_LOG_INVALID_PAGE: &str = "Invalid page number";
pub const T_COMMAND_LOG_INVALID_PAGE_RANGE: &str = "Please enter a value between {0} and {1}";
pub const T_COMMAND_LOG_EMPTY: &str = "No log entries to show";
pub const T_COMMAND_LOG_NOTIFY_ERROR_CONSOLE: &str = "Cannot toggle notifications for console";
pub const T_COMMAND_LOG_NOTIFY_ENABLED_TERM: &str = "Enabled";
pub const T_COMMAND_LOG_NOTIFY_DISABLED_TERM: &str = "Disabled";
pub const T_COMMAND_LOG_NOTIFY_CHANGED_STATE: &str = "{0} logging output";
pub const T_COMMAND_LOG_NOTIFY_ALREADY_ON: &str = "You are already receiving notifications";
pub const T_COMMAND_LOG_NOTIFY_ALREADY_OFF: &str = "You aren't currently receiving notifications";
pub const T_COMMAND_LOG_NOTIFY_INVALID_STATE: &str = "State unknown. Expecting {0} or {1}";
pub const T_COMMAND_LOG_SHOW_SEARCH: &str = "Showing recent actions for query {0}";
pub const T_COMMAND_LOG_SHOW_RECENT: &str = "Showing recent actions";
pub const T_COMMAND_LOG_SHOW_BY: &str = "Showing recent actions by {0}";
pub const T_COMMAND_LOG_SHOW_HISTORY: &str = "Showing history for {0} {1}";
pub const T_COMMAND_EXPORT_ERROR_TERM: &str = "Error";
pub const T_COMMAND_EXPORT_ALREADY_RUNNING: &str = "Another export process is already running";
pub const T_COMMAND_EXPORT_FILE_ALREADY_EXISTS: &str = "File {0} already exists";
pub const T_COMMAND_EXPORT_FILE_NOT_WRITABLE: &str = "File {0} is not writable";
pub const T_COMMAND_EXPORT_FILE_SUCCESS: &str = "Successfully exported to {0}";
pub const T_COMMAND_EXPORT_FILE_UNEXPECTED_ERROR_WRITING: &str = "An unexpected error occurred whilst writing to the file";
pub const T_COMMAND_EXPORT_WEB_EXPORT_CODE: &str = "Export code";
pub const T_COMMAND_EXPORT_WEB_IMPORT_COMMAND_DESCRIPTION: &str = "Use the following command to import";
pub const T_COMMAND_IMPORT_TERM: &str = "Import";
pub const T_COMMAND_IMPORT_ERROR_TERM: &str = "Error";
pub const T_COMMAND_IMPORT_ALREADY_RUNNING: &str = "Another import process is already running";
pub const T_COMMAND_IMPORT_FILE_DOESNT_EXIST: &str = "File {0} does not exist";
pub const T_COMMAND_IMPORT_FILE_NOT_READABLE: &str = "File {0} is not readable";
pub const T_COMMAND_IMPORT_FILE_UNEXPECTED_ERROR_READING: &str = "An unexpected error occurred whilst reading from the import file";
pub const T_COMMAND_IMPORT_FILE_CORRECT_FORMAT: &str = "is it the correct format?";
pub const T_COMMAND_IMPORT_WEB_UNABLE_TO_READ: &str = "Unable to read data using the given code";
pub const T_COMMAND_IMPORT_PROGRESS_PERCENT: &str = "{0}% complete";
pub const T_COMMAND_IMPORT_PROGRESS_OPERATIONS: &str = "{0}/{1} operations complete";
pub const T_COMMAND_IMPORT_STARTING: &str = "Starting import process";
pub const T_COMMAND_IMPORT_COMPLETED: &str = "COMPLETED";
pub const T_COMMAND_IMPORT_DURATION: &str = "took {0} seconds";
pub const T_COMMAND_BULKUPDATE_DISABLED: &str = "Bulk update functionality is disabled in the configuration file";
pub const T_COMMAND_BULKUPDATE_MUST_USE_CONSOLE: &str = "The bulk update command can only be used from the console";
pub const T_COMMAND_BULKUPDATE_INVALID_DATA_TYPE: &str = "Invalid type, was expecting {0}";
pub const T_COMMAND_BULKUPDATE_INVALID_CONSTRAINT: &str = "Invalid constraint {0}";
pub const T_COMMAND_BULKUPDATE_INVALID_CONSTRAINT_FORMAT: &str = "Constraints should be in the format {0}";
pub const T_COMMAND_BULKUPDATE_INVALID_COMPARISON: &str = "Invalid comparison operator {0}";
pub const T_COMMAND_BULKUPDATE_INVALID_COMPARISON_FORMAT: &str = "Expected one of the following: {0}";
pub const T_COMMAND_BULKUPDATE_QUEUED: &str = "Bulk update operation was queued";
pub const T_COMMAND_BULKUPDATE_CONFIRM: &str = "Run {0} to execute the update";
pub const T_COMMAND_BULKUPDATE_UNKNOWN_ID: &str = "Operation with id {0} does not exist or has expired";
pub const T_COMMAND_BULKUPDATE_STARTING: &str = "Running bulk update";
pub const T_COMMAND_BULKUPDATE_SUCCESS: &str = "Bulk update completed successfully";
pub const T_COMMAND_BULKUPDATE_SUCCESS_STATISTICS_NODES: &str = "Total affected nodes";
pub const T_COMMAND_BULKUPDATE_SUCCESS_STATISTICS_USERS: &str = "Total affected users";
pub const T_COMMAND_BULKUPDATE_SUCCESS_STATISTICS_GROUPS: &str = "Total affected groups";
pub const T_COMMAND_BULKUPDATE_FAILURE: &str = "Bulk update failed, check the console for errors";
pub const T_COMMAND_UPDATE_TASK_REQUEST: &str = "An update task has been requested, please wait";
pub const T_COMMAND_UPDATE_TASK_COMPLETE: &str = "Update task complete";
pub const T_COMMAND_UPDATE_TASK_PUSH_ATTEMPTING: &str = "Now attempting to push to other servers";
pub const T_COMMAND_UPDATE_TASK_PUSH_COMPLETE: &str = "Other servers were notified via {0} successfully";
pub const T_COMMAND_UPDATE_TASK_PUSH_ERROR: &str = "Error whilst pushing changes to other servers";
pub const T_COMMAND_UPDATE_TASK_PUSH_ERROR_NOT_SETUP: &str = "Cannot push changes to other servers as a messaging service has not been configured";
pub const T_COMMAND_RELOAD_CONFIG_SUCCESS: &str = "The configuration file was reloaded";
pub const T_COMMAND_RELOAD_CONFIG_RESTART_NOTE: &str = "some options will only apply after the server has restarted";
pub const T_COMMAND_TRANSLATIONS_SEARCHING: &str = "Searching for available translations, please wait...";
pub const T_COMMAND_TRANSLATIONS_SEARCHING_ERROR: &str = "Unable to obtain a list of available translations";
pub const T_COMMAND_TRANSLATIONS_INSTALLED_TRANSLATIONS: &str = "Installed Translations";
pub const T_COMMAND_TRANSLATIONS_AVAILABLE_TRANSLATIONS: &str = "Available Translations";
pub const T_COMMAND_TRANSLATIONS_PERCENT_TRANSLATED: &str = "{0}% translated";
pub const T_COMMAND_TRANSLATIONS_TRANSLATIONS_BY: &str = "by";
pub const T_COMMAND_TRANSLATIONS_INSTALLING: &str = "Installing translations, please wait...";
pub const T_COMMAND_TRANSLATIONS_DOWNLOAD_ERROR: &str = "Unable download translation for {0}";
pub const T_COMMAND_TRANSLATIONS_INSTALLING_SPECIFIC: &str = "Installing language {0}...";
pub const T_COMMAND_TRANSLATIONS_INSTALL_COMPLETE: &str = "Installation complete";
pub const T_COMMAND_TRANSLATIONS_DOWNLOAD_PROMPT: &str = "Use {0} to download and install up-to-date versions of these translations provided by the community";
pub const T_COMMAND_TRANSLATIONS_DOWNLOAD_OVERRIDE_WARNING: &str = "Please note that this will override any changes you've made for these languages";

/// `luckperms.usage.*`: descriptions and argument texts by key.
pub const USAGE: &[(&str, &str)] = &[
    ("user.description", "A set of commands for managing users within LuckPerms. (A 'user' in LuckPerms is just a player, and can refer to a UUID or username)"),
    ("group.description", "A set of commands for managing groups within LuckPerms. Groups are just collections of permission assignments that can be given to users. New groups are made using the 'creategroup' command."),
    ("track.description", "A set of commands for managing tracks within LuckPerms. Tracks are a ordered collection of groups which can be used for defining promotions and demotions."),
    ("log.description", "A set of commands for managing the logging functionality within LuckPerms."),
    ("sync.description", "Reloads all data from the plugins storage into memory, and applies any changes that are detected."),
    ("info.description", "Prints general information about the active plugin instance."),
    ("editor.description", "Creates a new web editor session"),
    ("editor.argument.type", "the types to load into the editor. ('all', 'users' or 'groups')"),
    ("editor.argument.filter", "permission to filter user entries by"),
    ("verbose.description", "Controls the plugins verbose permission check monitoring system."),
    ("verbose.argument.action", "whether to enable/disable logging, or to upload the logged output"),
    ("verbose.argument.filter", "the filter to match entries against"),
    ("verbose.argument.commandas", "the player/command to run"),
    ("tree.description", "Generates a tree view (ordered list hierarchy) of all permissions known to LuckPerms."),
    ("tree.argument.scope", "the root of the tree. specify \".\" to include all permissions"),
    ("tree.argument.player", "the name of an online player to check against"),
    ("search.description", "Searches for all of the users/groups with a specific permission"),
    ("search.argument.permission", "the permission to search for"),
    ("search.argument.page", "the page to view"),
    ("network-sync.description", "Sync changes with the storage and request that all other servers on the network do the same"),
    ("import.description", "Imports data from a (previously created) export file"),
    ("import.argument.file", "the file to import from"),
    ("import.argument.replace", "replace existing data instead of merging"),
    ("import.argument.upload", "upload the data from a previous export"),
    ("export.description", "Exports all permissions data to an 'export' file. Can be re-imported at a later time."),
    ("export.argument.file", "the file to export to"),
    ("export.argument.without-users", "exclude users from the export"),
    ("export.argument.without-groups", "exclude groups from the export"),
    ("export.argument.upload", "Upload all permission data to the webeditor. Can be re-imported at a later time."),
    ("reload-config.description", "Reload some of the config options"),
    ("bulk-update.description", "Execute bulk change queries on all data"),
    ("bulk-update.argument.data-type", "the type of data being changed. ('all', 'users' or 'groups')"),
    ("bulk-update.argument.action", "the action to perform on the data. ('update' or 'delete')"),
    ("bulk-update.argument.action-field", "the field to act upon. only required for 'update'. ('permission', 'server' or 'world')"),
    ("bulk-update.argument.action-value", "the value to replace with. only required for 'update'."),
    ("bulk-update.argument.constraint", "the constraints required for the update"),
    ("translations.description", "Manage translations"),
    ("translations.argument.install", "subcommand to install translations"),
    ("apply-edits.description", "Applies permission changes made from the web editor"),
    ("apply-edits.argument.code", "the unique code for the data"),
    ("apply-edits.argument.target", "who to apply the data to"),
    ("trust-editor.description", "Trusts an editor session to apply changes without a confirmation"),
    ("trust-editor.argument.id", "the id of the session to trust"),
    ("create-group.description", "Create a new group"),
    ("create-group.argument.name", "the name of the group"),
    ("create-group.argument.weight", "the weight of the group"),
    ("create-group.argument.display-name", "the display name of the group"),
    ("delete-group.description", "Delete a group"),
    ("delete-group.argument.name", "the name of the group"),
    ("list-groups.description", "List all groups on the platform"),
    ("list-groups.argument.page", "the page to view"),
    ("create-track.description", "Create a new track"),
    ("create-track.argument.name", "the name of the track"),
    ("delete-track.description", "Delete a track"),
    ("delete-track.argument.name", "the name of the track"),
    ("list-tracks.description", "List all tracks on the platform"),
    ("user-info.description", "Shows info about the user"),
    ("user-switchprimarygroup.description", "Switches the user's primary group"),
    ("user-switchprimarygroup.argument.group", "the group to switch to"),
    ("user-promote.description", "Promotes the user up a track"),
    ("user-promote.argument.track", "the track to promote the user up"),
    ("user-promote.argument.context", "the contexts to promote the user in"),
    ("user-promote.argument.dont-add-to-first", "only promote the user if they're already on the track"),
    ("user-demote.description", "Demotes the user down a track"),
    ("user-demote.argument.track", "the track to demote the user down"),
    ("user-demote.argument.context", "the contexts to demote the user in"),
    ("user-demote.argument.dont-remove-from-first", "prevent the user from being removed from the first group"),
    ("user-clone.description", "Clone the user"),
    ("user-clone.argument.user", "the name/uuid of the user to clone onto"),
    ("group-info.description", "Gives info about the group"),
    ("group-listmembers.description", "Show the users/groups who inherit from this group"),
    ("group-listmembers.argument.page", "the page to view"),
    ("group-listmembers.argument.context", "the context to filter members by"),
    ("group-setweight.description", "Set the groups weight"),
    ("group-setweight.argument.weight", "the weight to set"),
    ("group-set-display-name.description", "Set the groups display name"),
    ("group-set-display-name.argument.name", "the name to set"),
    ("group-set-display-name.argument.context", "the contexts to set the name in"),
    ("group-rename.description", "Rename the group"),
    ("group-rename.argument.name", "the new name"),
    ("group-clone.description", "Clone the group"),
    ("group-clone.argument.name", "the name of the group to clone onto"),
    ("holder-editor.description", "Opens the web permission editor"),
    ("holder-showtracks.description", "Lists the tracks that the object is on"),
    ("holder-clear.description", "Removes all permissions, parents and meta"),
    ("holder-clear.argument.context", "the contexts to filter by"),
    ("permission.description", "Edit permissions"),
    ("parent.description", "Edit inheritances"),
    ("meta.description", "Edit metadata values"),
    ("permission-info.description", "Lists the permission nodes the object has"),
    ("permission-info.argument.page", "the page to view"),
    ("permission-info.argument.sort-mode", "how to sort the entries"),
    ("permission-set.description", "Sets a permission for the object"),
    ("permission-set.argument.node", "the permission node to set"),
    ("permission-set.argument.value", "the value of the node"),
    ("permission-set.argument.context", "the contexts to add the permission in"),
    ("permission-unset.description", "Unsets a permission for the object"),
    ("permission-unset.argument.node", "the permission node to unset"),
    ("permission-unset.argument.context", "the contexts to remove the permission in"),
    ("permission-settemp.description", "Sets a permission for the object temporarily"),
    ("permission-settemp.argument.node", "the permission node to set"),
    ("permission-settemp.argument.value", "the value of the node"),
    ("permission-settemp.argument.duration", "the duration until the permission node expires"),
    ("permission-settemp.argument.temporary-modifier", "how the temporary permission should be applied"),
    ("permission-settemp.argument.context", "the contexts to add the permission in"),
    ("permission-unsettemp.description", "Unsets a temporary permission for the object"),
    ("permission-unsettemp.argument.node", "the permission node to unset"),
    ("permission-unsettemp.argument.duration", "the duration to subtract"),
    ("permission-unsettemp.argument.context", "the contexts to remove the permission in"),
    ("permission-check.description", "Checks to see if the object has a certain permission node"),
    ("permission-check.argument.node", "the permission node to check for"),
    ("permission-clear.description", "Clears all permissions"),
    ("permission-clear.argument.context", "the contexts to filter by"),
    ("parent-info.description", "Lists the groups that this object inherits from"),
    ("parent-info.argument.page", "the page to view"),
    ("parent-info.argument.sort-mode", "how to sort the entries"),
    ("parent-set.description", "Removes all other groups the object inherits already and adds them to the one given"),
    ("parent-set.argument.group", "the group to set to"),
    ("parent-set.argument.context", "the contexts to set the group in"),
    ("parent-add.description", "Sets another group for the object to inherit permissions from"),
    ("parent-add.argument.group", "the group to inherit from"),
    ("parent-add.argument.context", "the contexts to inherit the group in"),
    ("parent-remove.description", "Removes a previously set inheritance rule"),
    ("parent-remove.argument.group", "the group to remove"),
    ("parent-remove.argument.context", "the contexts to remove the group in"),
    ("parent-set-track.description", "Removes all other groups the object inherits from already on the given track and adds them to the one given"),
    ("parent-set-track.argument.track", "the track to set on"),
    ("parent-set-track.argument.group", "the group to set to, or a number relating to the position of the group on the given track"),
    ("parent-set-track.argument.context", "the contexts to set the group in"),
    ("parent-add-temp.description", "Sets another group for the object to inherit permissions from temporarily"),
    ("parent-add-temp.argument.group", "the group to inherit from"),
    ("parent-add-temp.argument.duration", "the duration of the group membership"),
    ("parent-add-temp.argument.temporary-modifier", "how the temporary permission should be applied"),
    ("parent-add-temp.argument.context", "the contexts to inherit the group in"),
    ("parent-remove-temp.description", "Removes a previously set temporary inheritance rule"),
    ("parent-remove-temp.argument.group", "the group to remove"),
    ("parent-remove-temp.argument.duration", "the duration to subtract"),
    ("parent-remove-temp.argument.context", "the contexts to remove the group in"),
    ("parent-clear.description", "Clears all parents"),
    ("parent-clear.argument.context", "the contexts to filter by"),
    ("parent-clear-track.description", "Clears all parents on a given track"),
    ("parent-clear-track.argument.track", "the track to remove on"),
    ("parent-clear-track.argument.context", "the contexts to filter by"),
    ("meta-info.description", "Shows all chat meta"),
    ("meta-set.description", "Sets a meta value"),
    ("meta-set.argument.key", "the key to set"),
    ("meta-set.argument.value", "the value to set"),
    ("meta-set.argument.context", "the contexts to add the meta pair in"),
    ("meta-unset.description", "Unsets a meta value"),
    ("meta-unset.argument.key", "the key to unset"),
    ("meta-unset.argument.context", "the contexts to remove the meta pair in"),
    ("meta-settemp.description", "Sets a meta value temporarily"),
    ("meta-settemp.argument.key", "the key to set"),
    ("meta-settemp.argument.value", "the value to set"),
    ("meta-settemp.argument.duration", "the duration until the meta value expires"),
    ("meta-settemp.argument.context", "the contexts to add the meta pair in"),
    ("meta-unsettemp.description", "Unsets a temporary meta value"),
    ("meta-unsettemp.argument.key", "the key to unset"),
    ("meta-unsettemp.argument.context", "the contexts to remove the meta pair in"),
    ("meta-addprefix.description", "Adds a prefix"),
    ("meta-addprefix.argument.priority", "the priority to add the prefix at"),
    ("meta-addprefix.argument.prefix", "the prefix string"),
    ("meta-addprefix.argument.context", "the contexts to add the prefix in"),
    ("meta-addsuffix.description", "Adds a suffix"),
    ("meta-addsuffix.argument.priority", "the priority to add the suffix at"),
    ("meta-addsuffix.argument.suffix", "the suffix string"),
    ("meta-addsuffix.argument.context", "the contexts to add the suffix in"),
    ("meta-setprefix.description", "Sets a prefix"),
    ("meta-setprefix.argument.priority", "the priority to set the prefix at"),
    ("meta-setprefix.argument.prefix", "the prefix string"),
    ("meta-setprefix.argument.context", "the contexts to set the prefix in"),
    ("meta-setsuffix.description", "Sets a suffix"),
    ("meta-setsuffix.argument.priority", "the priority to set the suffix at"),
    ("meta-setsuffix.argument.suffix", "the suffix string"),
    ("meta-setsuffix.argument.context", "the contexts to set the suffix in"),
    ("meta-removeprefix.description", "Removes a prefix"),
    ("meta-removeprefix.argument.priority", "the priority to remove the prefix at"),
    ("meta-removeprefix.argument.prefix", "the prefix string"),
    ("meta-removeprefix.argument.context", "the contexts to remove the prefix in"),
    ("meta-removesuffix.description", "Removes a suffix"),
    ("meta-removesuffix.argument.priority", "the priority to remove the suffix at"),
    ("meta-removesuffix.argument.suffix", "the suffix string"),
    ("meta-removesuffix.argument.context", "the contexts to remove the suffix in"),
    ("meta-addtemp-prefix.description", "Adds a prefix temporarily"),
    ("meta-addtemp-prefix.argument.priority", "the priority to add the prefix at"),
    ("meta-addtemp-prefix.argument.prefix", "the prefix string"),
    ("meta-addtemp-prefix.argument.duration", "the duration until the prefix expires"),
    ("meta-addtemp-prefix.argument.context", "the contexts to add the prefix in"),
    ("meta-addtemp-suffix.description", "Adds a suffix temporarily"),
    ("meta-addtemp-suffix.argument.priority", "the priority to add the suffix at"),
    ("meta-addtemp-suffix.argument.suffix", "the suffix string"),
    ("meta-addtemp-suffix.argument.duration", "the duration until the suffix expires"),
    ("meta-addtemp-suffix.argument.context", "the contexts to add the suffix in"),
    ("meta-settemp-prefix.description", "Sets a prefix temporarily"),
    ("meta-settemp-prefix.argument.priority", "the priority to set the prefix at"),
    ("meta-settemp-prefix.argument.prefix", "the prefix string"),
    ("meta-settemp-prefix.argument.duration", "the duration until the prefix expires"),
    ("meta-settemp-prefix.argument.context", "the contexts to set the prefix in"),
    ("meta-settemp-suffix.description", "Sets a suffix temporarily"),
    ("meta-settemp-suffix.argument.priority", "the priority to set the suffix at"),
    ("meta-settemp-suffix.argument.suffix", "the suffix string"),
    ("meta-settemp-suffix.argument.duration", "the duration until the suffix expires"),
    ("meta-settemp-suffix.argument.context", "the contexts to set the suffix in"),
    ("meta-removetemp-prefix.description", "Removes a temporary prefix"),
    ("meta-removetemp-prefix.argument.priority", "the priority to remove the prefix at"),
    ("meta-removetemp-prefix.argument.prefix", "the prefix string"),
    ("meta-removetemp-prefix.argument.context", "the contexts to remove the prefix in"),
    ("meta-removetemp-suffix.description", "Removes a temporary suffix"),
    ("meta-removetemp-suffix.argument.priority", "the priority to remove the suffix at"),
    ("meta-removetemp-suffix.argument.suffix", "the suffix string"),
    ("meta-removetemp-suffix.argument.context", "the contexts to remove the suffix in"),
    ("meta-clear.description", "Clears all meta"),
    ("meta-clear.argument.type", "the type of meta to remove"),
    ("meta-clear.argument.context", "the contexts to filter by"),
    ("track-info.description", "Gives info about the track"),
    ("track-editor.description", "Opens the web permission editor"),
    ("track-append.description", "Appends a group onto the end of the track"),
    ("track-append.argument.group", "the group to append"),
    ("track-insert.description", "Inserts a group at a given position along the track"),
    ("track-insert.argument.group", "the group to insert"),
    ("track-insert.argument.position", "the position to insert the group at (the first position on the track is 1)"),
    ("track-remove.description", "Removes a group from the track"),
    ("track-remove.argument.group", "the group to remove"),
    ("track-clear.description", "Clears the groups on the track"),
    ("track-rename.description", "Rename the track"),
    ("track-rename.argument.name", "the new name"),
    ("track-clone.description", "Clone the track"),
    ("track-clone.argument.name", "the name of the track to clone onto"),
    ("log-recent.description", "View recent actions"),
    ("log-recent.argument.user", "the name/uuid of the user to filter by"),
    ("log-recent.argument.page", "the page number to view"),
    ("log-search.description", "Search the log for an entry"),
    ("log-search.argument.query", "the query to search by"),
    ("log-search.argument.page", "the page number to view"),
    ("log-notify.description", "Toggle log notifications"),
    ("log-notify.argument.toggle", "whether to toggle on or off"),
    ("log-user-history.description", "View a user's history"),
    ("log-user-history.argument.user", "the name/uuid of the user"),
    ("log-user-history.argument.page", "the page number to view"),
    ("log-group-history.description", "View an group's history"),
    ("log-group-history.argument.group", "the name of the group"),
    ("log-group-history.argument.page", "the page number to view"),
    ("log-track-history.description", "View a track's history"),
    ("log-track-history.argument.track", "the name of the track"),
    ("log-track-history.argument.page", "the page number to view"),
    ("sponge.description", "Edit extra Sponge data"),
    ("sponge.argument.collection", "the collection to query"),
    ("sponge.argument.subject", "the subject to modify"),
    ("sponge-permission-info.description", "Shows info about the subject's permissions"),
    ("sponge-permission-info.argument.contexts", "the contexts to filter by"),
    ("sponge-permission-set.description", "Sets a permission for the Subject"),
    ("sponge-permission-set.argument.node", "the permission node to set"),
    ("sponge-permission-set.argument.tristate", "the value to set the permission to"),
    ("sponge-permission-set.argument.contexts", "the contexts to set the permission in"),
    ("sponge-permission-clear.description", "Clears the Subjects permissions"),
    ("sponge-permission-clear.argument.contexts", "the contexts to clear permissions in"),
    ("sponge-parent-info.description", "Shows info about the subject's parents"),
    ("sponge-parent-info.argument.contexts", "the contexts to filter by"),
    ("sponge-parent-add.description", "Adds a parent to the Subject"),
    ("sponge-parent-add.argument.collection", "the subject collection where the parent Subject is"),
    ("sponge-parent-add.argument.subject", "the name of the parent Subject"),
    ("sponge-parent-add.argument.contexts", "the contexts to add the parent in"),
    ("sponge-parent-remove.description", "Removes a parent from the Subject"),
    ("sponge-parent-remove.argument.collection", "the subject collection where the parent Subject is"),
    ("sponge-parent-remove.argument.subject", "the name of the parent Subject"),
    ("sponge-parent-remove.argument.contexts", "the contexts to remove the parent in"),
    ("sponge-parent-clear.description", "Clears the Subjects parents"),
    ("sponge-parent-clear.argument.contexts", "the contexts to clear parents in"),
    ("sponge-option-info.description", "Shows info about the subject's options"),
    ("sponge-option-info.argument.contexts", "the contexts to filter by"),
    ("sponge-option-set.description", "Sets an option for the Subject"),
    ("sponge-option-set.argument.key", "the key to set"),
    ("sponge-option-set.argument.value", "the value to set the key to"),
    ("sponge-option-set.argument.contexts", "the contexts to set the option in"),
    ("sponge-option-unset.description", "Unsets an option for the Subject"),
    ("sponge-option-unset.argument.key", "the key to unset"),
    ("sponge-option-unset.argument.contexts", "the contexts to unset the key in"),
    ("sponge-option-clear.description", "Clears the Subjects options"),
    ("sponge-option-clear.argument.contexts", "the contexts to clear options in"),
];

/// The usage text for `key` (`permission-set.description`, `permission-set.argument.node`).
pub fn usage_text(key: &str) -> &'static str {
    USAGE.iter().find(|(k, _)| *k == key).map(|(_, v)| *v).unwrap_or("")
}

/// Fills `{0}`, `{1}` ... of a template. The text starts in colour `base` (a legacy code letter) and returns to it after
/// every argument, as a component's arguments keep their own style while the rest of the text keeps the parent's.
pub fn tr(template: &str, base: char, args: &[String]) -> String {
    let mut out = String::new();
    out.push('\u{a7}');
    out.push(base);
    let chars: Vec<char> = template.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '{' {
            if let Some(j) = chars[i + 1..].iter().position(|c| *c == '}') {
                let inner: String = chars[i + 1..i + 1 + j].iter().collect();
                if let Ok(n) = inner.parse::<usize>() {
                    if let Some(a) = args.get(n) {
                        out.push_str(a);
                        out.push('\u{a7}');
                        out.push(base);
                    }
                    i += j + 2;
                    continue;
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}


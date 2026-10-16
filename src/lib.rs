//! Nasus: curador das suas leituras. O binário `nasus` (TUI, web, bot, MCP)
//! e o `nasus-app` (janela desktop) usam os mesmos módulos.

pub mod app;
pub mod atualizar;
pub mod bot;
pub mod codigo_barras;
pub mod dados;
pub mod db;
pub mod desktop;
pub mod input;
pub mod mcp;
pub mod model;
pub mod openlibrary;
pub mod ui;
pub mod web;

<?php
header('Content-Type: application/json');
try {
    $db = new PDO('sqlite:' . getenv('SCOREBOARD_DB'));
    $db->exec('PRAGMA foreign_keys = ON');
    echo json_encode(['ok' => true, 'teams' => (int) $db->query('SELECT COUNT(*) FROM teams')->fetchColumn()]);
} catch (Throwable $e) {
    http_response_code(503);
    echo json_encode(['ok' => false]);
}

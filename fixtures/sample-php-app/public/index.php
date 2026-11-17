<?php
$db = new PDO('sqlite:' . getenv('SCOREBOARD_DB'));
$db->setAttribute(PDO::ATTR_ERRMODE, PDO::ERRMODE_EXCEPTION);
$db->exec('PRAGMA foreign_keys = ON');
if ($_SERVER['REQUEST_METHOD'] === 'POST' && isset($_POST['team'])) {
    $db->prepare('INSERT INTO scores(team_id, points) VALUES (?, 1)')->execute([(int) $_POST['team']]);
    header('Location: /');
    exit;
}
$rows = $db->query('SELECT t.id, t.name, COALESCE(SUM(s.points),0) AS pts FROM teams t LEFT JOIN scores s ON s.team_id = t.id GROUP BY t.id')->fetchAll(PDO::FETCH_ASSOC);
?><!doctype html><meta charset="utf-8"><title>Sample Scoreboard</title>
<h1>Sample Scoreboard</h1>
<?php foreach ($rows as $r): ?>
<form method="post"><input type="hidden" name="team" value="<?= (int) $r['id'] ?>"><?= htmlspecialchars($r['name']) ?>: <?= (int) $r['pts'] ?> <button>+1</button></form>
<?php endforeach; ?>

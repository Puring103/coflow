$cli='C:/Users/wtlll/Desktop/projects/coflow/target/debug/coflow.exe'
$root='C:/Users/wtlll/AppData/Local/Temp/coflow-skill-eval-20261028/batch'
function Run($argv,$name) {
 $output=& $cli @argv --project $root --json 2>&1
 $code=$LASTEXITCODE
 "COMMAND: coflow $($argv -join ' ') --project $root --json`nEXIT: $code`n$output" | Set-Content "$root/$name.log" -Encoding utf8
 return ($output | ConvertFrom-Json)
}
$r=Run @('data','list','--type','Item','--where','price < 100','--fields','price') 'read'
if(!$r.data_scope.complete -or $r.result.truncated){throw 'incomplete scope'}
$req=@{protocol_version=1;kind='operations';ops=@(@{op='set_where';selector=@{type='Item';filter='price < 100'};path=@(@{kind='field';name='price'});value=@{kind='expression';source='100'};expected_count=$r.result.total})}
$req | ConvertTo-Json -Depth 15 | Set-Content "$root/changes.json" -Encoding utf8
# 仅追加无业务影响的注释，模拟另一写入者在读取后修改源码。
Add-Content "$root/data/items.cfd" "`n# 并发写入者注释：保留此内容" -Encoding utf8
Run @('apply','--input',"$root/changes.json",'--rules','--if-revision',$r.revision) 'conflict' | ConvertTo-Json -Depth 15
$r=Run @('data','list','--type','Item','--where','price < 100','--fields','price') 'reread'
if(!$r.data_scope.complete -or $r.result.truncated -or $r.result.total -ne 2 -or ($r.result.items.id -join ',') -ne 'shield,sword'){throw 'scope changed, review needed'}
Run @('apply','--input',"$root/changes.json",'--rules','--if-revision',$r.revision) 'apply' | ConvertTo-Json -Depth 15
Run @('data','list','--type','Item','--fields','price') 'after' | Out-Null
Run @('data','list','--type','Item','--where','price < 100','--fields','price') 'remaining' | Out-Null
Run @('check','--rules') 'check' | Out-Null

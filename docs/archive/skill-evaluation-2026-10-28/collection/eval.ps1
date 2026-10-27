$cli='C:/Users/wtlll/Desktop/projects/coflow/target/debug/coflow.exe'
$root='C:/Users/wtlll/AppData/Local/Temp/coflow-skill-eval-20261028'
function Run($dir,$argv,$name) {
 $output=& $cli @argv --project "$root/$dir" --json 2>&1
 $code=$LASTEXITCODE
 "COMMAND: coflow $($argv -join ' ') --project $root/$dir --json`nEXIT: $code`n$output" | Set-Content "$root/$dir/$name.log" -Encoding utf8
 return ($output | ConvertFrom-Json)
}
foreach($dir in @('collection','batch')) {
 Run $dir @('project','inspect') 'inspect' | Out-Null
 Run $dir @('project','files') 'files' | Out-Null
 Run $dir @('schema','symbols','--kind','type') 'symbols' | Out-Null
 Run $dir @('context','--type','Item') 'context' | Out-Null
}
$r=Run 'collection' @('data','get','--type','Item','--id','sword','--view','both') 'before'
$request=@{protocol_version=1;kind='operations';ops=@(
 @{op='set_field';record=@{type='Item';id='sword'};path=@(@{kind='field';name='price'});value=@{kind='expression';source='100'};expected=@{kind='expression';source='90'}},
 @{op='array_insert';record=@{type='Item';id='sword'};path=@(@{kind='field';name='tags'});index=0;value=@{kind='expression';source='"weapon"'}},
 @{op='dict_set';record=@{type='Item';id='sword'};path=@(@{kind='field';name='labels'});key=@{kind='expression';source='"category"'};value=@{kind='expression';source='"equipment"'}}
)}
$request | ConvertTo-Json -Depth 20 | Set-Content "$root/collection/changes.json" -Encoding utf8
Run 'collection' @('apply','--input',"$root/collection/changes.json",'--rules','--if-revision',$r.revision) 'apply' | ConvertTo-Json -Depth 20
Run 'collection' @('data','get','--type','Item','--id','sword','--view','both') 'after' | Out-Null
$r=Run 'batch' @('data','list','--type','Item','--where','price < 100','--fields','price') 'before'
$r | ConvertTo-Json -Depth 20
